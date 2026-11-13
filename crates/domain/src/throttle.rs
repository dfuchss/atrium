//! Sliding-window rate limits per key (account), shared by the core and the adapters.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// Allows at most `max_events` per key within any `window_ms` (caller's clock, in ms).
#[derive(Debug)]
pub struct RateLimiter<K> {
    max_events: usize,
    window_ms: u64,
    recent: HashMap<K, VecDeque<u64>>,
}

impl<K: Hash + Eq> RateLimiter<K> {
    pub fn new(max_events: usize, window_ms: u64) -> RateLimiter<K> {
        RateLimiter {
            max_events,
            window_ms,
            recent: HashMap::new(),
        }
    }

    /// Records an event of `key` at `now_ms` if the limit allows it; false = drop it.
    pub fn allow(&mut self, key: K, now_ms: u64) -> bool {
        let stamps = self.recent.entry(key).or_default();
        while stamps
            .front()
            .is_some_and(|&stamp| now_ms.saturating_sub(stamp) >= self.window_ms)
        {
            stamps.pop_front();
        }
        if stamps.len() >= self.max_events {
            return false;
        }
        stamps.push_back(now_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_each_key_within_the_window() {
        let mut limiter = RateLimiter::new(2, 1_000);
        assert!(limiter.allow('a', 0));
        assert!(limiter.allow('a', 10));
        assert!(!limiter.allow('a', 999));
        assert!(limiter.allow('b', 999), "keys are independent");
        assert!(limiter.allow('a', 1_000), "the first event left the window");
        assert!(!limiter.allow('a', 1_005));
        assert!(limiter.allow('a', 2_010));
    }

    #[test]
    fn dropped_events_do_not_extend_the_window() {
        let mut limiter = RateLimiter::new(1, 2_000);
        assert!(limiter.allow(1, 0));
        assert!(!limiter.allow(1, 1_500));
        assert!(limiter.allow(1, 2_000));
    }
}
