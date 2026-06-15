//! Unit tests for Plan 13 T1: `StuckDetector`.
//!
//! Three spec scenarios:
//!   1. Moving >4 u/s every sample → never stuck.
//!   2. Completely stalled → `Mild` after ~1 s, `Hard` after ~5 s.
//!   3. Resumes moving → resets to `None`.

use brain::recover::{StuckDetector, StuckLevel};
use glam::Vec3;

// Feed `n` updates at `dt` seconds each, all at the same `pos`.
fn stall_for(det: &mut StuckDetector, pos: Vec3, n: usize, dt: f32) -> StuckLevel {
    let mut level = StuckLevel::None;
    for _ in 0..n {
        level = det.update(pos, dt);
    }
    level
}

// Feed `n` updates at `dt` seconds each, advancing `pos` by `speed*dt` on +X.
fn move_for(det: &mut StuckDetector, start: Vec3, speed: f32, n: usize, dt: f32) -> StuckLevel {
    let mut level = StuckLevel::None;
    for i in 0..n {
        let pos = start + Vec3::new(speed * dt * i as f32, 0.0, 0.0);
        level = det.update(pos, dt);
    }
    level
}

/// Moving at 300 u/s (>>4 u/s deadband) should never trip stuck.
#[test]
fn moving_fast_never_stuck() {
    let mut det = StuckDetector::new();
    // 30 ticks at 0.1 s = 3 s of fast movement.
    let level = move_for(&mut det, Vec3::ZERO, 300.0, 30, 0.1);
    assert_eq!(level, StuckLevel::None, "fast movement: never stuck");
}

/// Moving at 3 u/s (<4 u deadband in 1 s) → stuck within each sample window.
#[test]
fn slow_creep_within_deadband_trips_stuck() {
    let mut det = StuckDetector::new();
    // 3 u/s * 1 s = 3 u < DEADBAND (4).
    // First 1-second checkpoint (tick 10) sets `last_sample_pos` — no stuck_secs yet.
    // Second checkpoint (tick 20) sees only 3 u moved → stuck_secs=1 → Mild.
    // Use 25 ticks (2.5 s) to guarantee the second checkpoint fires.
    let level = move_for(&mut det, Vec3::ZERO, 3.0, 25, 0.1);
    assert!(
        level != StuckLevel::None,
        "barely moving (3 u/s) should be stuck: got {level:?}"
    );
}

/// Completely stalled (same position) → `Mild` after ~1 s, `Hard` after ~5 s.
#[test]
fn stalled_becomes_mild_then_hard() {
    let mut det = StuckDetector::new();
    let pos = Vec3::new(100.0, 200.0, 0.0);

    // The first sample sets `last_sample_pos` without incrementing stuck_secs.
    // After 1 more second stalled → stuck_secs=1 → Mild.
    // Use 0.1 s ticks; first sample fires after 10 ticks.

    // Feed 20 ticks = 2 s; first checkpoint at ~1 s sets last_sample_pos.
    // Second checkpoint at ~2 s increments stuck_secs to 1 → Mild.
    let mut level = StuckLevel::None;
    for _ in 0..20 {
        level = det.update(pos, 0.1);
    }
    assert!(
        matches!(level, StuckLevel::Mild),
        "after 2 s stalled → Mild; got {level:?}"
    );

    // Feed 50 more ticks = 5 more seconds (total ~7 s stalled → stuck_secs ≥ 5).
    for _ in 0..50 {
        level = det.update(pos, 0.1);
    }
    assert!(
        matches!(level, StuckLevel::Hard),
        "after 7 s stalled → Hard; got {level:?}"
    );
}

/// Resumes moving after being stuck → level resets to `None` at the next sample.
#[test]
fn resumes_moving_resets_stuck() {
    let mut det = StuckDetector::new();
    let pos = Vec3::new(100.0, 0.0, 0.0);

    // Get to Mild.
    stall_for(&mut det, pos, 20, 0.1);

    // Now move 1000 u in 10 ticks (100 u/tick >> deadband).
    let level = move_for(&mut det, pos, 1000.0, 15, 0.1);
    assert_eq!(
        level,
        StuckLevel::None,
        "resuming movement resets stuck to None; got {level:?}"
    );
}

/// `reset()` clears all stuck state immediately.
#[test]
fn reset_clears_state() {
    let mut det = StuckDetector::new();
    let pos = Vec3::ZERO;
    stall_for(&mut det, pos, 20, 0.1); // reach Mild
    det.reset();
    // After reset a single tick at the same pos should be None (timer cleared).
    let level = det.update(pos, 0.1);
    assert_eq!(level, StuckLevel::None, "reset: should be None after reset");
}
