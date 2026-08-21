//! Token bucket rate limiting.
//!
//! A bucket starts full with `capacity` tokens and refills continuously at
//! `refill_rate` tokens per second, never exceeding `capacity`. Each request
//! costs some number of tokens; if enough are available they are taken
//! immediately, otherwise the request is rejected. There is no queueing —
//! callers decide what to do with a rejection (retry later, return 429, drop).

use std::time::Instant;

pub struct TokenBucket {
    capacity: f64,
    refill_rate: f64,
    tokens: f64,
    // `last_update` is measured in seconds since `epoch`, so the same
    // acquire logic works for both real-time use (via `try_acquire`) and
    // deterministic simulation with caller-supplied timestamps (via
    // `try_acquire_at`).
    last_update: f64,
    epoch: Instant,
}

impl TokenBucket {
    /// Creates a full bucket. Panics if `capacity` or `refill_rate` are not
    /// positive, since a bucket that never fills or never refills is
    /// almost certainly a caller mistake rather than an intended limiter.
    pub fn new(capacity: f64, refill_rate: f64) -> Self {
        assert!(capacity > 0.0, "capacity must be positive");
        assert!(refill_rate > 0.0, "refill_rate must be positive");
        TokenBucket {
            capacity,
            refill_rate,
            tokens: capacity,
            last_update: 0.0,
            epoch: Instant::now(),
        }
    }

    /// Attempts to take `cost` tokens using the wall clock. Returns `true`
    /// and deducts the tokens if enough were available, otherwise leaves
    /// the bucket untouched and returns `false`.
    pub fn try_acquire(&mut self, cost: f64) -> bool {
        let now = self.epoch.elapsed().as_secs_f64();
        self.try_acquire_at(now, cost)
    }

    /// Same as `try_acquire`, but the caller supplies "now" as seconds
    /// since the bucket was created. This is what makes the bucket
    /// testable and simulatable without sleeping in real time — feed it
    /// a recorded or synthetic timeline instead of the wall clock.
    pub fn try_acquire_at(&mut self, now: f64, cost: f64) -> bool {
        let elapsed = (now - self.last_update).max(0.0);
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_update = now;

        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }

    /// Tokens currently available, as of the wall clock.
    pub fn available(&mut self) -> f64 {
        let now = self.epoch.elapsed().as_secs_f64();
        self.try_acquire_at(now, 0.0);
        self.tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_full_and_drains() {
        let mut bucket = TokenBucket::new(3.0, 1.0);
        assert!(bucket.try_acquire_at(0.0, 1.0));
        assert!(bucket.try_acquire_at(0.0, 1.0));
        assert!(bucket.try_acquire_at(0.0, 1.0));
        assert!(!bucket.try_acquire_at(0.0, 1.0));
    }

    #[test]
    fn refills_over_time() {
        let mut bucket = TokenBucket::new(2.0, 1.0);
        assert!(bucket.try_acquire_at(0.0, 2.0));
        assert!(!bucket.try_acquire_at(0.5, 1.0));
        // one second later, one token should have regenerated
        assert!(bucket.try_acquire_at(1.5, 1.0));
    }

    #[test]
    fn never_exceeds_capacity() {
        let mut bucket = TokenBucket::new(2.0, 10.0);
        // huge gap in time should still cap at capacity, not overflow it
        assert!(bucket.try_acquire_at(1000.0, 2.0));
        assert!(!bucket.try_acquire_at(1000.0, 0.1));
    }

    #[test]
    fn rejects_non_positive_config() {
        let result = std::panic::catch_unwind(|| TokenBucket::new(0.0, 1.0));
        assert!(result.is_err());
    }
}

/// Sliding window rate limiting.
///
/// Where a token bucket tracks a single continuously refilling balance, a
/// sliding window limiter remembers the timestamp of every request that
/// landed inside the trailing `window` and simply counts them. That makes
/// the guarantee exact — never more than `limit` requests in any `window`
/// seconds, full stop — at the cost of O(limit) memory instead of O(1). It
/// also means, unlike a token bucket, that a burst right at the start of a
/// window doesn't buy back capacity early: it has to fully age out.
pub mod sliding_window {
    use std::collections::VecDeque;
    use std::time::Instant;

    pub struct SlidingWindowLimiter {
        limit: usize,
        window: f64,
        timestamps: VecDeque<f64>,
        epoch: Instant,
    }

    impl SlidingWindowLimiter {
        /// Creates a limiter allowing at most `limit` requests in any
        /// trailing `window` seconds. Panics if `limit` is zero or
        /// `window` is not positive, for the same reason `TokenBucket`
        /// rejects non-positive configuration: it's almost certainly a
        /// caller mistake rather than an intended "always deny" limiter.
        pub fn new(limit: usize, window: f64) -> Self {
            assert!(limit > 0, "limit must be positive");
            assert!(window > 0.0, "window must be positive");
            SlidingWindowLimiter {
                limit,
                window,
                timestamps: VecDeque::new(),
                epoch: Instant::now(),
            }
        }

        /// Attempts to record a request using the wall clock. Returns
        /// `true` if it fits within the limit for the trailing window,
        /// otherwise `false` and the window is left untouched aside from
        /// evicting entries that have aged out.
        pub fn try_acquire(&mut self) -> bool {
            let now = self.epoch.elapsed().as_secs_f64();
            self.try_acquire_at(now)
        }

        /// Same as `try_acquire`, but the caller supplies "now" as seconds
        /// since the limiter was created, for deterministic testing and
        /// simulation. Timestamps must be non-decreasing across calls.
        pub fn try_acquire_at(&mut self, now: f64) -> bool {
            self.evict_before(now - self.window);

            if self.timestamps.len() < self.limit {
                self.timestamps.push_back(now);
                true
            } else {
                false
            }
        }

        /// Requests still allowed in the current window, as of the wall
        /// clock.
        pub fn remaining(&mut self) -> usize {
            let now = self.epoch.elapsed().as_secs_f64();
            self.evict_before(now - self.window);
            self.limit - self.timestamps.len()
        }

        /// Drops timestamps that are now outside the window, i.e. at or
        /// before `cutoff` (= now - window).
        fn evict_before(&mut self, cutoff: f64) {
            while let Some(&front) = self.timestamps.front() {
                if front <= cutoff {
                    self.timestamps.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn allows_up_to_limit_within_window() {
            let mut limiter = SlidingWindowLimiter::new(3, 1.0);
            assert!(limiter.try_acquire_at(0.0));
            assert!(limiter.try_acquire_at(0.1));
            assert!(limiter.try_acquire_at(0.2));
            assert!(!limiter.try_acquire_at(0.3));
        }

        #[test]
        fn old_requests_age_out_of_the_window() {
            let mut limiter = SlidingWindowLimiter::new(2, 1.0);
            assert!(limiter.try_acquire_at(0.0));
            assert!(limiter.try_acquire_at(0.5));
            assert!(!limiter.try_acquire_at(0.9));
            // the request at t=0.0 is now outside the trailing 1s window
            assert!(limiter.try_acquire_at(1.1));
        }

        #[test]
        fn boundary_is_exclusive_of_the_window_edge() {
            let mut limiter = SlidingWindowLimiter::new(1, 1.0);
            assert!(limiter.try_acquire_at(0.0));
            // exactly one window later, the first request has fully aged out
            assert!(limiter.try_acquire_at(1.0));
        }

        #[test]
        fn remaining_reflects_evictions() {
            let mut limiter = SlidingWindowLimiter::new(2, 1.0);
            assert!(limiter.try_acquire_at(0.0));
            assert!(limiter.try_acquire_at(0.0));
            assert_eq!(limiter.remaining(), 0);
        }

        #[test]
        fn rejects_non_positive_config() {
            let result = std::panic::catch_unwind(|| SlidingWindowLimiter::new(0, 1.0));
            assert!(result.is_err());
        }
    }
}
