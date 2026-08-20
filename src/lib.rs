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
