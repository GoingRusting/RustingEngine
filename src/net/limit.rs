//! Per-peer rate limits for what clients send the host.

use super::PeerId;
use std::collections::BTreeMap;
use std::time::Instant;

/// A token bucket per peer: each peer may send `burst` messages at once,
/// then `per_second` on average. Ask it before handling a client message
/// and drop (or count against the sender) the ones it refuses.
#[derive(Clone, Debug)]
pub struct RateLimit {
    per_second: f64,
    burst: f64,
    buckets: BTreeMap<PeerId, (f64, Instant)>,
}

impl RateLimit {
    pub fn new(per_second: f64, burst: u32) -> Self {
        Self {
            per_second,
            burst: f64::from(burst),
            buckets: BTreeMap::new(),
        }
    }

    /// Takes one message from `peer` at `now`; `false` when it is over its
    /// rate.
    pub fn allow(&mut self, peer: PeerId, now: Instant) -> bool {
        let (tokens, last) =
            self.buckets.entry(peer).or_insert((self.burst, now));
        let earned = now.saturating_duration_since(*last).as_secs_f64();
        *tokens = (*tokens + earned * self.per_second).min(self.burst);
        *last = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Forgets `peer`, on `NetEvent::Disconnected`.
    pub fn forget(&mut self, peer: PeerId) {
        self.buckets.remove(&peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn each_peer_gets_a_burst_then_its_rate() {
        let mut limit = RateLimit::new(10.0, 3);
        let start = Instant::now();
        let allowed = |limit: &mut RateLimit, peer, at| {
            (0..5).filter(|_| limit.allow(peer, at)).count()
        };
        assert_eq!(allowed(&mut limit, 1, start), 3);
        assert_eq!(allowed(&mut limit, 2, start), 3); // its own bucket
        let later = start + Duration::from_millis(200);
        assert_eq!(allowed(&mut limit, 1, later), 2);
        // A long pause refills only up to the burst.
        let much_later = start + Duration::from_secs(60);
        assert_eq!(allowed(&mut limit, 1, much_later), 3);
        limit.forget(1);
        assert_eq!(allowed(&mut limit, 1, much_later), 3);
    }
}
