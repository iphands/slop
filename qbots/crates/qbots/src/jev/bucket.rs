//! # qbots::jev::bucket — the fleet input-token bucket (Plan 72 T5)
//!
//! **Reserve at admission, true up afterwards, and let the balance go negative.** The real token
//! count (`usage.input_tokens`) only exists after the response, so charging post hoc would let
//! `max_concurrent` requests be admitted above the budget before the first charge lands. Instead
//! each admission reserves an estimate (`chars / 4` of the request body), and the true-up adds
//! back `reserved − actual` — which may push the balance below zero, delaying the next
//! admission. Admission requires a **positive** balance; the reservation itself may overdraw.
//!
//! The clock is an explicit argument, so the tests are exact without sleeping.

use tokio::time::Instant;

/// A token bucket refilled at `rate` tokens/s up to `capacity`.
#[derive(Clone, Debug)]
pub struct Bucket {
    rate: f64,
    capacity: f64,
    tokens: f64,
    last: Instant,
}

impl Bucket {
    /// A full bucket. `capacity` is the burst allowance (the hub uses one second of `rate`).
    pub fn new(rate: f64, capacity: f64, now: Instant) -> Self {
        Self {
            rate,
            capacity,
            tokens: capacity,
            last: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + dt * self.rate).min(self.capacity);
        self.last = now;
    }

    /// Admit and reserve `est` tokens if the balance is positive; `false` = refused.
    pub fn try_reserve(&mut self, now: Instant, est: f64) -> bool {
        self.refill(now);
        if self.tokens > 0.0 {
            self.tokens -= est;
            true
        } else {
            false
        }
    }

    /// Replace a reservation with the server's actual count (`reserved − actual` back).
    pub fn true_up(&mut self, reserved: f64, actual: f64) {
        self.tokens += reserved - actual;
    }

    /// The current balance (may be negative), refilled to `now`.
    pub fn balance(&mut self, now: Instant) -> f64 {
        self.refill(now);
        self.tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn jev_hub_bucket_reserve_true_up_and_overdraw() {
        let t0 = Instant::now();
        let mut b = Bucket::new(1000.0, 1000.0, t0);
        // Positive balance admits even a reservation larger than the balance (overdraw).
        assert!(b.try_reserve(t0, 1500.0));
        assert!((b.balance(t0) + 500.0).abs() < 1e-9);
        assert!(!b.try_reserve(t0, 10.0), "negative balance refuses");
        // Half a second refills 500 → exactly 0: still not positive.
        assert!(!b.try_reserve(t0 + Duration::from_millis(500), 10.0));
        assert!(b.try_reserve(t0 + Duration::from_millis(501), 10.0));
        // The server says the 10-token estimate was really 400: the true-up overdraws again.
        b.true_up(10.0, 400.0);
        assert!(b.balance(t0 + Duration::from_millis(501)) < -380.0);
    }

    #[test]
    fn jev_hub_bucket_limits_to_the_rate_over_ten_seconds() {
        // 1000 tok/s, 350-token asks offered every 10 ms for 10 s: admitted tokens must stay
        // within rate·10 s + one burst + one overdraw.
        let t0 = Instant::now();
        let mut b = Bucket::new(1000.0, 1000.0, t0);
        let mut admitted = 0.0;
        for i in 0..1000 {
            let now = t0 + Duration::from_millis(i * 10);
            if b.try_reserve(now, 350.0) {
                admitted += 350.0;
            }
        }
        assert!(
            admitted <= 1000.0 * 10.0 + 1000.0 + 350.0,
            "admitted {admitted}"
        );
        assert!(
            admitted >= 1000.0 * 10.0 - 350.0,
            "the bucket is not over-strict: {admitted}"
        );
    }

    #[test]
    fn jev_hub_bucket_true_up_with_real_usage_changes_the_admission_rate() {
        // Estimates of 100 but real usage of 400: true-up must cut admissions ~4×.
        let run = |actual: f64| {
            let t0 = Instant::now();
            let mut b = Bucket::new(1000.0, 1000.0, t0);
            let mut n = 0;
            for i in 0..1000 {
                let now = t0 + Duration::from_millis(i * 10);
                if b.try_reserve(now, 100.0) {
                    b.true_up(100.0, actual);
                    n += 1;
                }
            }
            n
        };
        let (honest, under) = (run(100.0), run(400.0));
        assert!(
            honest > 3 * under,
            "honest {honest} vs underestimated {under}"
        );
    }
}
