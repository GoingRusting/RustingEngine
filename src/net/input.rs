//! A host-side buffer of client inputs by tick, which absorbs jitter.
//!
//! Clients stamp each input with the tick it is for, a few ticks ahead of
//! the host ([`super::clock::ClockSync::input_tick`]; the margin is the
//! input delay). Inputs that arrive early wait here for their tick;
//! [`InputBuffer::take`] hands the host each peer's input for the tick it
//! runs, repeating the last one for a peer whose input is late or lost.

use super::PeerId;
use std::collections::BTreeMap;

/// Ticks ahead of the host an input may be stamped. Further ones are
/// refused, so a client cannot fill the buffer.
pub const MAX_AHEAD: u64 = 120;

/// What [`InputBuffer::take`] found for one peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickInput<I> {
    pub peer: PeerId,
    pub input: I,
    /// `false` when the input for this tick had not arrived and the last
    /// one was repeated.
    pub fresh: bool,
}

#[derive(Clone, Debug)]
struct Peer<I> {
    waiting: BTreeMap<u64, I>,
    last: Option<I>,
    late: u64,
}

/// Inputs by peer and tick. Keep one on the host.
#[derive(Clone, Debug)]
pub struct InputBuffer<I> {
    peers: BTreeMap<PeerId, Peer<I>>,
    /// The next tick [`InputBuffer::take`] serves.
    next: u64,
}

impl<I: Clone> InputBuffer<I> {
    /// A buffer whose first [`InputBuffer::take`] is for `tick`.
    pub fn new(tick: u64) -> Self {
        Self {
            peers: BTreeMap::new(),
            next: tick,
        }
    }

    /// Stores `peer`'s input for `tick`. Refuses one for a tick already
    /// taken (too late) or more than [`MAX_AHEAD`] ticks out. A second
    /// input for one tick replaces the first.
    pub fn push(
        &mut self,
        peer: PeerId,
        tick: u64,
        input: I,
    ) -> Result<(), String> {
        if tick < self.next {
            self.peer(peer).late += 1;
            return Err(format!(
                "peer {peer}: input for tick {tick} came too late"
            ));
        }
        if tick - self.next > MAX_AHEAD {
            return Err(format!(
                "peer {peer}: input for tick {tick} is too far ahead"
            ));
        }
        self.peer(peer).waiting.insert(tick, input);
        Ok(())
    }

    fn peer(&mut self, peer: PeerId) -> &mut Peer<I> {
        self.peers.entry(peer).or_insert_with(|| Peer {
            waiting: BTreeMap::new(),
            last: None,
            late: 0,
        })
    }

    /// Each peer's input for the next tick, in peer order, and advances.
    /// A peer with no input yet at all is left out.
    pub fn take(&mut self) -> (u64, Vec<TickInput<I>>) {
        let tick = self.next;
        self.next += 1;
        let inputs = self
            .peers
            .iter_mut()
            .filter_map(|(&peer, state)| {
                // Older entries cannot be left, but drop them anyway.
                state.waiting = state.waiting.split_off(&tick);
                let fresh = state.waiting.remove(&tick);
                let is_fresh = fresh.is_some();
                if let Some(input) = fresh {
                    state.last = Some(input);
                }
                let input = state.last.clone()?;
                Some(TickInput {
                    peer,
                    input,
                    fresh: is_fresh,
                })
            })
            .collect();
        (tick, inputs)
    }

    /// Inputs `peer` has waiting: how far ahead it runs, in ticks. Near
    /// zero means the next jitter spike makes it late.
    pub fn buffered(&self, peer: PeerId) -> usize {
        self.peers.get(&peer).map_or(0, |state| state.waiting.len())
    }

    /// Inputs from `peer` that arrived after their tick was taken.
    pub fn late(&self, peer: PeerId) -> u64 {
        self.peers.get(&peer).map_or(0, |state| state.late)
    }

    /// Forgets `peer`, on `NetEvent::Disconnected`.
    pub fn forget(&mut self, peer: PeerId) {
        self.peers.remove(&peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_inputs_wait_and_missing_ones_repeat_the_last() {
        let mut buffer = InputBuffer::new(10);
        // Peer 1 sends 2 ticks ahead, out of order; peer 2 skips tick 11.
        buffer.push(1, 11, 'b').unwrap();
        buffer.push(1, 10, 'a').unwrap();
        buffer.push(2, 10, 'x').unwrap();
        buffer.push(2, 12, 'z').unwrap();
        assert_eq!(buffer.buffered(1), 2);

        let input = |peer, input, fresh| TickInput { peer, input, fresh };
        assert_eq!(
            buffer.take(),
            (10, vec![input(1, 'a', true), input(2, 'x', true)])
        );
        assert_eq!(
            buffer.take(),
            (11, vec![input(1, 'b', true), input(2, 'x', false)])
        );
        assert_eq!(
            buffer.take(),
            (12, vec![input(1, 'b', false), input(2, 'z', true)])
        );

        assert!(buffer.push(2, 11, 'y').is_err());
        assert_eq!(buffer.late(2), 1);
        assert!(buffer.push(1, 13 + MAX_AHEAD + 1, 'q').is_err());
        buffer.push(3, 20, 'n').unwrap();
        assert_eq!(buffer.take().1.len(), 2); // peer 3 has no input yet
        buffer.forget(1);
        assert_eq!(buffer.take().1, [input(2, 'z', false)]);
    }
}
