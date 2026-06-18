//! Unit tests for Plan 13 T1–T3.
//!
//! T1 — `StuckDetector`:
//!   1. Moving >4 u/s every sample → never stuck.
//!   2. Completely stalled → `Mild` after ~1 s, `Hard` after ~5 s.
//!   3. Resumes moving → resets to `None`.
//!
//! T2 — `find_best_direction` fan-out:
//!   4. Open field → returns Some (non-zero score).
//!   5. All solid (startsolid in every direction) → returns None.
//!   6. Wall blocking forward; clear to the side → side direction wins.
//!
//! T3 — `Recovery::evaluate`:
//!   7. Not stuck, has nav target → None.
//!   8. Mild stuck, no wall ahead (cm=None) → Jump.
//!   9. Hard stuck, not engaging → BackOffThenRepath.
//!  10. Hard stuck, engaging → Strafe (not BackOffThenRepath).

use brain::recover::{Recovery, RecoveryAction, StuckDetector, StuckLevel};
use glam::Vec3;
use world::CollisionModel;

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

// ── T2: find_best_direction ───────────────────────────────────────────────────

/// `half_space([1,0,0], -100000)`: front (x >= -100000) is empty.
/// Every trace from anywhere near the origin stays in the empty region.
fn all_clear_model() -> CollisionModel {
    CollisionModel::half_space([1.0, 0.0, 0.0], -100_000.0)
}

/// `half_space([1,0,0], 100000)`: back (x < 100000) is solid.
/// Origin [0,0,0] is solid → all traces are startsolid → skipped → returns None.
fn all_solid_model() -> CollisionModel {
    CollisionModel::half_space([1.0, 0.0, 0.0], 100_000.0)
}

/// Wall at x=50: `half_space([-1,0,0], -50)` → front (x <= 50) empty, back (x > 50) solid.
/// Blocks the forward (+x) direction. Wall is far enough that the player hull (±16u in x)
/// does not extend into solid space from origin.
fn wall_at_x50_model() -> CollisionModel {
    CollisionModel::half_space([-1.0, 0.0, 0.0], -50.0)
}

/// Open field: `find_best_direction` should return `Some` with a positive score.
#[test]
fn find_best_direction_open_field_returns_some() {
    let cm = all_clear_model();
    let result = brain::recover::find_best_direction(&cm, Vec3::ZERO, 0.0);
    assert!(result.is_some(), "open field should find a direction");
    let (_yaw, score) = result.unwrap();
    assert!(score > 0.0, "open-field score should be positive");
}

/// All solid (startsolid in every direction) → returns `None`.
#[test]
fn find_best_direction_all_blocked_returns_none() {
    let cm = all_solid_model();
    let result = brain::recover::find_best_direction(&cm, Vec3::ZERO, 0.0);
    assert!(
        result.is_none(),
        "all-solid model: all directions startsolid → None"
    );
}

/// Wall at x=50 (view_yaw=0, facing +x): side directions score higher than the blocked forward.
#[test]
fn find_best_direction_wall_ahead_picks_side() {
    let cm = wall_at_x50_model();
    // Bot at origin, view_yaw=0 (facing +x where wall is at x=50).
    // Player hull is ±16 in x; origin x=0 + hull = [-16,16] is well within x<=50 empty region.
    let result = brain::recover::find_best_direction(&cm, Vec3::ZERO, 0.0);
    assert!(
        result.is_some(),
        "side directions should still be clear despite wall ahead"
    );
    let (_yaw, score) = result.unwrap();
    // Forward (0°) traces from 0 to 256 in +x, hits wall at hull-edge x=34 (50-16=34).
    // Score ~= 34 * 0.5 (ledge) = ~17. Side direction (+y) reaches 256 unblocked → ~128.
    // Best should be ≥ the side score.
    assert!(
        score > 20.0,
        "wall blocks forward; side should win with score > 20; got {score}"
    );
}

// ── T3: Recovery::evaluate ────────────────────────────────────────────────────

/// Get to Mild stuck in the detector without a real CollisionModel.
fn reach_mild(rec: &mut Recovery, pos: Vec3) {
    // 20 ticks at 0.1 s = 2 s stalled → Mild (needs 2 checkpoints to fire stuck_secs=1).
    for _ in 0..20 {
        let _ = rec.evaluate(pos, 0.1, None, 0.0, true, false);
    }
}

/// Advance until the detector reaches Hard, returning the first Hard-level action.
/// `BackOffThenRepath` resets the detector, so we must capture within the loop.
/// Mild stuck now also strafes, so for the engaging case (which strafes at Hard too)
/// we only accept a strafe once enough time has passed to be Hard (≥3.5 s = 35 ticks).
fn first_hard_action(rec: &mut Recovery, pos: Vec3, engaging: bool) -> RecoveryAction {
    for i in 0..80 {
        let action = rec.evaluate(pos, 0.1, None, 0.0, true, engaging);
        if !engaging {
            if matches!(action, RecoveryAction::BackOffThenRepath) {
                return action;
            }
        } else if i >= 35 && matches!(action, RecoveryAction::Strafe { .. }) {
            return action;
        }
    }
    RecoveryAction::None // should not happen — indicates test logic error
}

/// Not stuck yet → `None`.
#[test]
fn evaluate_not_stuck_returns_none() {
    let mut rec = Recovery::new();
    let pos = Vec3::new(100.0, 0.0, 0.0);
    // First tick: nothing accumulated.
    let action = rec.evaluate(pos, 0.1, None, 0.0, true, false);
    assert_eq!(action, RecoveryAction::None, "first tick: not stuck");
}

/// Mild stuck → `Strafe` (pogo-jump recovery was removed; a side-step breaks the
/// stall without bouncing the bot in place or dropping it off a ledge).
#[test]
fn evaluate_mild_stuck_strafes() {
    let mut rec = Recovery::new();
    let pos = Vec3::new(50.0, 0.0, 0.0);
    reach_mild(&mut rec, pos);
    let action = rec.evaluate(pos, 0.1, None, 0.0, true, false);
    assert!(
        matches!(action, RecoveryAction::Strafe { .. }),
        "Mild stuck → Strafe; got {action:?}"
    );
}

/// Hard stuck, not engaging → `BackOffThenRepath`.
#[test]
fn evaluate_hard_stuck_not_engaging_backs_off() {
    let mut rec = Recovery::new();
    let pos = Vec3::new(50.0, 0.0, 0.0);
    // Note: BackOffThenRepath resets the detector, so we must capture within the loop.
    let action = first_hard_action(&mut rec, pos, false);
    assert_eq!(
        action,
        RecoveryAction::BackOffThenRepath,
        "Hard stuck + not engaging → BackOffThenRepath"
    );
}

/// Hard stuck, engaging (in combat) → `Strafe`, not `BackOffThenRepath`.
#[test]
fn evaluate_hard_stuck_while_engaging_strafes() {
    let mut rec = Recovery::new();
    let pos = Vec3::new(50.0, 0.0, 0.0);
    // Engaging=true: hard stuck should strafe, not abandon nav.
    let action = first_hard_action(&mut rec, pos, true);
    assert!(
        matches!(action, RecoveryAction::Strafe { .. }),
        "Hard stuck while engaging → Strafe (not BackOffThenRepath); got {action:?}"
    );
}
