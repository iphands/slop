//! Self-inflicted reply-delay measurement (Plan 57).
//!
//! Quake 2's scoreboard "ping" is **not** network RTT — the server computes it as the
//! average, over the last 16 frames, of `(realtime it received the client's clc_move
//! acking frame N) − (senttime it stamped when it sent frame N)`
//! (`vendor/yquake2/src/server/sv_user.c:686-696`, `sv_main.c:131-164`). That number
//! therefore folds in the client's own *reply delay*: how long the bot sat on a frame
//! before its next outgoing packet.
//!
//! [`SendTiming`] measures exactly that reply delay locally, so we can prove the
//! ack-on-frame re-phasing works independent of the server's reported ping (which
//! `crates/qbots/src/status.rs` reads). It records the [`Instant`] each server frame
//! arrived, keyed by `serverframe`, and when we send the `clc_move` acking that frame it
//! reports the phase delay `send_time − arrival_time`. In the fixed 10 Hz free-running
//! design that delay averages ~50 ms (0–100 ms uniform); acking on arrival drives it to
//! ~0 plus scheduler jitter.
//!
//! The core is pure and clock-injected (callers pass `now`) so it is deterministic under
//! test — no wall clock is read here.

use std::time::{Duration, Instant};

/// Ring size — mirrors the server's `LATENCY_COUNTS` (`server/header/server.h:34`), so we
/// remember arrivals for as many frames as the server averages over.
const RING: usize = 16;

/// Phase delays at or above this are counted as `late` — a starved select loop or a
/// missed ack cycle, not the ~0 ms we expect from acking on arrival.
const LATE_THRESHOLD_MS: f64 = 40.0;

/// EMA smoothing factor for the reported phase delay (higher = more responsive).
const EMA_ALPHA: f64 = 0.2;

/// One remembered frame arrival.
#[derive(Clone, Copy)]
struct Arrival {
    serverframe: i32,
    at: Instant,
}

/// Rolling measurement of the frame-arrival→ack-sent phase delay (Plan 57).
///
/// Feed it [`SendTiming::on_frame`] when a `svc_frame` is decoded and
/// [`SendTiming::on_ack_sent`] when the `clc_move` acking a frame goes out; read
/// [`SendTiming::snapshot`] periodically for an `EVT send_timing` log line.
#[derive(Default)]
pub struct SendTiming {
    ring: [Option<Arrival>; RING],
    ema_ms: f64,
    max_ms: f64,
    sends: u64,
    late: u64,
}

/// A read-only view of [`SendTiming`] for logging.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SendTimingStats {
    /// Exponential moving average of the phase delay, milliseconds.
    pub ema_ms: f64,
    /// Worst phase delay observed, milliseconds.
    pub max_ms: f64,
    /// Number of acks measured.
    pub sends: u64,
    /// Number of acks whose phase delay was ≥ [`LATE_THRESHOLD_MS`].
    pub late: u64,
}

impl SendTiming {
    /// A fresh measurement with an empty ring.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that server frame `serverframe` arrived at `now`.
    ///
    /// Keyed by `serverframe & (RING-1)`; a newer frame in the same slot evicts the older
    /// one, matching the server's own `frame_latency[]` addressing.
    pub fn on_frame(&mut self, serverframe: i32, now: Instant) {
        if serverframe < 0 {
            return;
        }
        let slot = (serverframe as usize) & (RING - 1);
        self.ring[slot] = Some(Arrival {
            serverframe,
            at: now,
        });
    }

    /// Record that the `clc_move` acking `acked_serverframe` was sent at `now`.
    ///
    /// Returns the phase delay `now − arrival(acked)` if that frame's arrival is still in
    /// the ring, updating the EMA/max/late accounting. Returns `None` when the frame is
    /// unknown or has been evicted (e.g. the very first ack, or after a long gap).
    pub fn on_ack_sent(&mut self, acked_serverframe: i32, now: Instant) -> Option<Duration> {
        if acked_serverframe < 0 {
            return None;
        }
        let slot = (acked_serverframe as usize) & (RING - 1);
        let arrival = self.ring[slot]?;
        if arrival.serverframe != acked_serverframe {
            // Slot was reused by a different frame — the one we acked is gone.
            return None;
        }
        // `now` should be ≥ arrival, but guard against a non-monotonic caller.
        let delay = now.saturating_duration_since(arrival.at);
        let ms = delay.as_secs_f64() * 1000.0;

        self.sends += 1;
        if self.sends == 1 {
            self.ema_ms = ms;
        } else {
            self.ema_ms = EMA_ALPHA * ms + (1.0 - EMA_ALPHA) * self.ema_ms;
        }
        if ms > self.max_ms {
            self.max_ms = ms;
        }
        if ms >= LATE_THRESHOLD_MS {
            self.late += 1;
        }
        Some(delay)
    }

    /// Current rolling stats for logging.
    pub fn snapshot(&self) -> SendTimingStats {
        SendTimingStats {
            ema_ms: self.ema_ms,
            max_ms: self.max_ms,
            sends: self.sends,
            late: self.late,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_delay_is_send_minus_arrival() {
        let mut t = SendTiming::new();
        let base = Instant::now();
        t.on_frame(100, base);
        let delay = t
            .on_ack_sent(100, base + Duration::from_millis(7))
            .expect("frame 100 is in the ring");
        assert_eq!(delay, Duration::from_millis(7));
        let s = t.snapshot();
        assert_eq!(s.sends, 1);
        assert!((s.ema_ms - 7.0).abs() < 1e-6);
        assert!((s.max_ms - 7.0).abs() < 1e-6);
        assert_eq!(s.late, 0);
    }

    #[test]
    fn unknown_or_evicted_frame_returns_none() {
        let mut t = SendTiming::new();
        let base = Instant::now();
        // Never recorded.
        assert!(t.on_ack_sent(5, base).is_none());
        // Recorded, then evicted by a same-slot newer frame (5 and 5+RING collide).
        t.on_frame(5, base);
        t.on_frame(5 + RING as i32, base + Duration::from_millis(1));
        assert!(
            t.on_ack_sent(5, base + Duration::from_millis(2)).is_none(),
            "frame 5 was overwritten by frame 5+RING in the same slot"
        );
        // The newer frame still resolves.
        assert!(t
            .on_ack_sent(5 + RING as i32, base + Duration::from_millis(3))
            .is_some());
    }

    #[test]
    fn ring_wraps_over_many_frames() {
        let mut t = SendTiming::new();
        let base = Instant::now();
        // Push 4 rings' worth; only the last RING frames should still resolve.
        for f in 0..(RING as i32 * 4) {
            t.on_frame(f, base + Duration::from_millis(f as u64));
        }
        let newest = RING as i32 * 4 - 1;
        assert!(t
            .on_ack_sent(newest, base + Duration::from_millis(newest as u64))
            .is_some());
        // A frame from an earlier ring is long gone.
        assert!(t.on_ack_sent(0, base).is_none());
    }

    #[test]
    fn late_acks_are_counted_and_max_tracked() {
        let mut t = SendTiming::new();
        let base = Instant::now();
        // A fast ack, then a slow (late) one.
        t.on_frame(1, base);
        t.on_ack_sent(1, base + Duration::from_millis(3));
        t.on_frame(2, base);
        t.on_ack_sent(2, base + Duration::from_millis(60));
        let s = t.snapshot();
        assert_eq!(s.sends, 2);
        assert_eq!(s.late, 1, "the 60 ms ack is late (>= 40 ms)");
        assert!((s.max_ms - 60.0).abs() < 1e-6);
    }

    #[test]
    fn negative_serverframe_is_ignored() {
        let mut t = SendTiming::new();
        let base = Instant::now();
        t.on_frame(-1, base);
        assert!(t.on_ack_sent(-1, base).is_none());
        assert_eq!(t.snapshot().sends, 0);
    }

    #[test]
    fn non_monotonic_send_time_does_not_panic() {
        let mut t = SendTiming::new();
        let base = Instant::now() + Duration::from_millis(100);
        t.on_frame(1, base);
        // Send "before" arrival — saturates to zero rather than underflowing.
        let d = t.on_ack_sent(1, base - Duration::from_millis(10));
        assert_eq!(d, Some(Duration::ZERO));
    }
}
