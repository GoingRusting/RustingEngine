//! Clock synchronization: a client's estimate of the host's tick, and the
//! host's measure of how early each client's inputs arrive.
//!
//! The client pings; the host answers with its tick; the client takes
//! half the round trip as the one-way delay. Of the last few samples it
//! trusts the one with the shortest round trip, which queueing delayed
//! least.

use super::{NetSession, PeerId, HOST};
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::time::{Duration, Instant};

/// Marks a clock message, so game messages on the same session pass by.
const MAGIC: &[u8; 4] = b"\0clk";
const PING: u8 = 0;
const PONG: u8 = 1;
/// Samples kept by [`ClockSync`].
const SAMPLES: usize = 8;

/// The client's view of the host's clock.
#[derive(Clone, Debug)]
pub struct ClockSync {
    base: Instant,
    step: Duration,
    /// `(round trip, host tick minus local tick)` of recent pongs.
    samples: VecDeque<(Duration, f64)>,
}

impl ClockSync {
    /// `step` is the fixed tick length, the same on both ends.
    pub fn new(step: Duration) -> Self {
        Self {
            base: Instant::now(),
            step,
            samples: VecDeque::new(),
        }
    }

    /// Sends a ping to the host. Send one every second or so; more while
    /// there are fewer than a handful of samples.
    pub fn ping(&self, session: &NetSession) -> io::Result<()> {
        let sent = self.base.elapsed().as_nanos() as u64;
        let mut bytes = MAGIC.to_vec();
        bytes.push(PING);
        bytes.extend(sent.to_le_bytes());
        session.send(HOST, &bytes)
    }

    /// Reads a message from the host at local tick `local_tick` (with
    /// fraction, for frames between ticks). Returns `false` for a message
    /// that is not a pong.
    pub fn accept(
        &mut self,
        from: PeerId,
        bytes: &[u8],
        local_tick: f64,
    ) -> bool {
        let Some(body) = bytes
            .strip_prefix(MAGIC)
            .and_then(|body| body.strip_prefix(&[PONG]))
            .filter(|body| from == HOST && body.len() == 16)
        else {
            return false;
        };
        let sent = u64::from_le_bytes(body[..8].try_into().unwrap());
        let host_tick = u64::from_le_bytes(body[8..].try_into().unwrap());
        let now = self.base.elapsed();
        let Some(round_trip) = now.checked_sub(Duration::from_nanos(sent))
        else {
            return true; // from the future: not ours
        };
        let one_way = round_trip.as_secs_f64() / 2.0 / self.step.as_secs_f64();
        let offset = host_tick as f64 + one_way - local_tick;
        if self.samples.len() == SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((round_trip, offset));
        true
    }

    fn best(&self) -> Option<(Duration, f64)> {
        self.samples.iter().copied().min_by_key(|(rtt, _)| *rtt)
    }

    /// The round trip of the best recent sample.
    pub fn round_trip(&self) -> Option<Duration> {
        self.best().map(|(rtt, _)| rtt)
    }

    /// The host's tick now, at local tick `local_tick`; `None` before the
    /// first pong.
    pub fn host_tick(&self, local_tick: f64) -> Option<f64> {
        self.best().map(|(_, offset)| local_tick + offset)
    }

    /// The tick to stamp an input with so it reaches the host `margin`
    /// ticks before the host runs that tick: the host's tick plus one
    /// one-way trip plus the margin, rounded up.
    pub fn input_tick(&self, local_tick: f64, margin: f64) -> Option<u64> {
        let (rtt, offset) = self.best()?;
        let one_way = rtt.as_secs_f64() / 2.0 / self.step.as_secs_f64();
        Some((local_tick + offset + one_way + margin).ceil().max(0.0) as u64)
    }
}

/// Host: answers a client's ping with `host_tick`. Returns `false` for a
/// message that is not a ping.
pub fn answer(
    session: &NetSession,
    from: PeerId,
    bytes: &[u8],
    host_tick: u64,
) -> bool {
    let Some(sent) = bytes
        .strip_prefix(MAGIC)
        .and_then(|body| body.strip_prefix(&[PING]))
        .filter(|sent| sent.len() == 8)
    else {
        return false;
    };
    let mut reply = MAGIC.to_vec();
    reply.push(PONG);
    reply.extend(sent);
    reply.extend(host_tick.to_le_bytes());
    // A client that left cannot be answered; nothing to do.
    let _ = session.send(from, &reply);
    true
}

/// Host: how many ticks early each client's inputs arrive (negative:
/// late), smoothed. Use it to tell a client to send further ahead.
#[derive(Clone, Debug, Default)]
pub struct TickOffsets {
    leads: BTreeMap<PeerId, f64>,
}

impl TickOffsets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an input stamped `input_tick` arriving at `host_tick`.
    pub fn observe(&mut self, peer: PeerId, input_tick: u64, host_tick: u64) {
        let lead = input_tick as f64 - host_tick as f64;
        let smoothed = self.leads.entry(peer).or_insert(lead);
        *smoothed += (lead - *smoothed) * 0.1;
    }

    /// The smoothed lead of `peer`'s inputs, in ticks.
    pub fn lead(&self, peer: PeerId) -> Option<f64> {
        self.leads.get(&peer).copied()
    }

    /// Forgets `peer`, on `NetEvent::Disconnected`.
    pub fn forget(&mut self, peer: PeerId) {
        self.leads.remove(&peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::NetEvent;

    fn message(session: &NetSession) -> (PeerId, Vec<u8>) {
        match session.poll().into_iter().last() {
            Some(NetEvent::Message { from, bytes }) => (from, bytes),
            other => panic!("expected a message, got {other:?}"),
        }
    }

    #[test]
    fn a_client_estimates_the_host_tick_and_input_lead() {
        let (host, clients) = NetSession::loopback(1);
        let step = Duration::from_millis(10);
        let mut clock = ClockSync::new(step);
        assert_eq!(clock.host_tick(0.0), None);

        // The host is at tick 500 when the client is at tick 20.
        clock.ping(&clients[0]).unwrap();
        let (from, ping) = message(&host);
        assert!(answer(&host, from, &ping, 500));
        let (from, pong) = message(&clients[0]);
        assert!(clock.accept(from, &pong, 20.0));
        let rtt = clock.round_trip().unwrap();
        assert!(rtt < step * 10, "{rtt:?}");
        let host_now = clock.host_tick(25.0).unwrap();
        assert!((505.0..506.0).contains(&host_now), "{host_now}");
        let input = clock.input_tick(25.0, 2.0).unwrap();
        assert!((507..=508).contains(&input), "{input}");

        // A slower sample does not replace the better one.
        clock.samples.push_back((Duration::from_secs(1), 900.0));
        assert!((505.0..506.0).contains(&clock.host_tick(25.0).unwrap()));

        // Foreign messages pass by.
        assert!(!clock.accept(HOST, b"game", 0.0));
        assert!(!clock.accept(1, &pong, 0.0));
        assert!(!answer(&host, 1, b"game", 0));

        let mut offsets = TickOffsets::new();
        offsets.observe(1, 105, 100);
        offsets.observe(1, 102, 101);
        let lead = offsets.lead(1).unwrap();
        assert!((4.0..5.0).contains(&lead), "{lead}");
        offsets.forget(1);
        assert_eq!(offsets.lead(1), None);
    }
}
