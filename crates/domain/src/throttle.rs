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

    /// Forgets the events of `key`: its next events count from zero.
    pub fn reset(&mut self, key: &K) {
        self.recent.remove(key);
    }
}

/// Why a new in-app window was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRefusal {
    TooManyOpen,
    TooFast,
}

/// Admission of in-app windows per key (account): a cap on windows open at once and a rate
/// limit on opening, so a page cannot flood the screen with `window.open`.
#[derive(Debug)]
pub struct WindowQuota<K> {
    max_open: usize,
    opening: RateLimiter<K>,
}

impl<K: Hash + Eq> WindowQuota<K> {
    pub fn new(max_open: usize, max_per_window: usize, window_ms: u64) -> WindowQuota<K> {
        WindowQuota {
            max_open,
            opening: RateLimiter::new(max_per_window, window_ms),
        }
    }

    /// Admits a window for `key` that has `open_now` windows, recording the opening. A window
    /// refused for the cap does not use up the rate limit.
    pub fn admit(&mut self, key: K, open_now: usize, now_ms: u64) -> Result<(), WindowRefusal> {
        if open_now >= self.max_open {
            return Err(WindowRefusal::TooManyOpen);
        }
        if self.opening.allow(key, now_ms) {
            Ok(())
        } else {
            Err(WindowRefusal::TooFast)
        }
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

    #[test]
    fn reset_forgets_the_events_of_one_key() {
        let mut limiter = RateLimiter::new(1, 1_000);
        assert!(limiter.allow('a', 0));
        assert!(limiter.allow('b', 0));
        limiter.reset(&'a');
        assert!(limiter.allow('a', 10));
        assert!(!limiter.allow('b', 10));
    }

    #[test]
    fn window_quota_caps_open_windows_and_opening_rate() {
        let mut quota = WindowQuota::new(2, 3, 10_000);
        assert_eq!(quota.admit(1, 2, 0), Err(WindowRefusal::TooManyOpen));
        assert_eq!(quota.admit(1, 0, 0), Ok(()));
        assert_eq!(quota.admit(1, 1, 100), Ok(()));
        assert_eq!(
            quota.admit(1, 0, 200),
            Ok(()),
            "the cap refusal cost nothing"
        );
        assert_eq!(quota.admit(1, 0, 300), Err(WindowRefusal::TooFast));
        assert_eq!(quota.admit(2, 0, 300), Ok(()), "accounts are independent");
        assert_eq!(quota.admit(1, 0, 10_000), Ok(()));
    }
}
