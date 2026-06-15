//! Unit tests for `brain::steer` — T1 and T2 of Plan 12.

use brain::steer::{move_from_world_dir, view_forward, view_right, Steering, YAW_SPEED_BASE};
use glam::Vec3;

// ── T1: change_yaw ────────────────────────────────────────────────────────────

#[test]
fn change_yaw_shortest_arc_positive() {
    // From 0°, ideal = +179°. Shortest arc = +179 (not -181).
    let mut s = Steering::new(0);
    let dt = 1.0; // 1 s → max_step = YAW_SPEED_BASE
    let result = s.change_yaw(179.0, dt);
    // At 720 dps and 1 s dt, full 179° is reachable in one step.
    assert!((result - 179.0).abs() < 0.01, "expected ~179, got {result}");
}

#[test]
fn change_yaw_shortest_arc_negative() {
    // From 0°, ideal = -179°. Shortest arc = -179 (not +181).
    let mut s = Steering::new(0);
    let result = s.change_yaw(-179.0, 1.0);
    assert!(
        (result - (-179.0)).abs() < 0.01,
        "expected ~-179, got {result}"
    );
}

#[test]
fn change_yaw_shortest_arc_chooses_small_side() {
    // From 0°, ideal = +181°. The shortest arc is −179° (go left), not +181°.
    let mut s = Steering::new(0);
    let result = s.change_yaw(181.0, 1.0);
    // After 1s at 720 dps, we step -179° from 0 → should land near -179.
    assert!(
        result < 0.0,
        "expected negative (shortest arc), got {result}"
    );
    assert!(result > -180.0, "should not overshoot -180");
}

#[test]
fn change_yaw_clamps_at_yaw_speed_times_dt() {
    // With dt = 0.1 and YAW_SPEED_BASE = 720, max step = 72°.
    let mut s = Steering::new(0);
    let dt = 0.1;
    // Ideal is 90° away (unambiguous positive arc) — should step exactly 72°.
    let result = s.change_yaw(90.0, dt);
    let expected_step = YAW_SPEED_BASE * dt;
    assert!(
        (result - expected_step).abs() < 0.01,
        "expected step of {expected_step}, got {result}"
    );
}

#[test]
fn change_yaw_never_overshoots_ideal() {
    // Small remaining diff — must not overshoot.
    let mut s = Steering::new(0);
    s.set_view_yaw(89.5);
    let result = s.change_yaw(90.0, 0.1); // only 0.5° remaining, max step = 72°
    assert!((result - 90.0).abs() < 0.01, "overshot: got {result}");
}

#[test]
fn change_yaw_skill_scaling_monotonic() {
    // Higher combat skill → faster turn per dt. Compare magnitudes (direction can vary).
    let dt = 0.1;
    let ideal = 90.0; // unambiguous +90° arc
    let deltas: Vec<f32> = (0u8..=4)
        .map(|skill| {
            let mut s = Steering::new(skill);
            s.change_yaw(ideal, dt).abs()
        })
        .collect();
    for i in 0..deltas.len() - 1 {
        assert!(
            deltas[i + 1] >= deltas[i],
            "skill {} turned less than skill {}: {:.1} vs {:.1}",
            i + 1,
            i,
            deltas[i + 1],
            deltas[i]
        );
    }
}

#[test]
fn change_yaw_wraps_at_180() {
    // Accumulating yaw past 180° should wrap rather than growing unbounded.
    let mut s = Steering::new(4); // fast turn
                                  // Do many small steps pushing past 180°.
    for _ in 0..20 {
        s.change_yaw(360.0, 0.1);
    }
    let yaw = s.view_yaw();
    assert!(yaw >= -180.0 && yaw < 180.0, "yaw out of range: {yaw}");
}

// ── T2: move_from_world_dir ───────────────────────────────────────────────────

#[test]
fn move_from_world_dir_facing_move_dir_gives_full_forward() {
    // Bot faces exactly the direction it wants to move → (1, 0).
    let dir = Vec3::new(1.0, 0.0, 0.0); // move +X
    let yaw = 0.0; // facing +X
    let (fwd, side) = move_from_world_dir(dir, yaw, true);
    assert!(
        (fwd - 1.0).abs() < 0.01,
        "forward should be ~1.0, got {fwd}"
    );
    assert!(side.abs() < 0.01, "side should be ~0.0, got {side}");
}

#[test]
fn move_from_world_dir_facing_90_off_throttles_forward() {
    // Bot faces +X but wants to move +Y (90° off).
    let dir = Vec3::new(0.0, 1.0, 0.0); // move +Y
    let yaw = 0.0; // facing +X (forward = +X, right = +Y wait no...)
                   // view_forward(0) = (cos0, sin0, 0) = (1, 0, 0)
                   // view_right(0) = (sin0, -cos0, 0) = (0, -1, 0)
                   // dot(dir=(0,1,0), fwd=(1,0,0)) = 0 → forward = 0
                   // dot(dir=(0,1,0), right=(0,-1,0)) = -1 → side = -1
    let (fwd, side) = move_from_world_dir(dir, yaw, true);
    // face_then_go: align = fwd.max(0) = 0 → out_fwd = |0| * 0 = 0
    assert!(
        fwd.abs() < 0.01,
        "forward should throttle to 0 at 90° off, got {fwd}"
    );
    assert!(
        (side + 1.0).abs() < 0.01,
        "side should be ~-1 (strafe left in Q2), got {side}"
    );
}

#[test]
fn move_from_world_dir_facing_away_does_not_moonwalk() {
    // Bot faces -X but wants to move +X (facing away).
    let dir = Vec3::new(1.0, 0.0, 0.0); // move +X
    let yaw = 180.0; // facing -X
                     // view_forward(180) = (cos π, sin π, 0) = (-1, 0, 0)
                     // dot(dir=(1,0,0), fwd=(-1,0,0)) = -1 → fwd = -1
    let (fwd, side) = move_from_world_dir(dir, yaw, true);
    // align = (-1).max(0) = 0 → forward = 0 (no moonwalking)
    assert!(
        fwd.abs() < 0.01,
        "should not reverse-walk when facing away, got {fwd}"
    );
    let _ = side; // side direction doesn't matter here
}

#[test]
fn move_from_world_dir_face_then_go_false_allows_full_strafe() {
    // Without face_then_go, bot can full-strafe perpendicular.
    let dir = Vec3::new(0.0, 1.0, 0.0); // move +Y
    let yaw = 0.0; // facing +X
                   // view_right(0) = (0, -1, 0), dot(+Y, -Y) = -1 → side = -1
    let (fwd, side) = move_from_world_dir(dir, yaw, false);
    assert!(
        fwd.abs() < 0.01,
        "no forward when moving perpendicular, got {fwd}"
    );
    assert!(
        (side + 1.0).abs() < 0.01,
        "side should be -1 for +Y move with yaw 0, got {side}"
    );
}

#[test]
fn move_from_world_dir_diagonal_magnitude_le_one() {
    // A 45° world move dir decomposed with face_then_go=false should have magnitude ≤ 1.
    let dir = Vec3::new(1.0, 1.0, 0.0).normalize();
    let yaw = 45.0; // facing the diagonal — forward ≈ 1, side ≈ 0
    let (fwd, side) = move_from_world_dir(dir, yaw, false);
    let mag = (fwd * fwd + side * side).sqrt();
    assert!(mag <= 1.01, "magnitude must be ≤ 1.0, got {mag}");
}

#[test]
fn move_from_world_dir_zero_vector_gives_zero() {
    let (fwd, side) = move_from_world_dir(Vec3::ZERO, 45.0, true);
    assert_eq!(fwd, 0.0);
    assert_eq!(side, 0.0);
}

// ── view_forward / view_right helpers ────────────────────────────────────────

#[test]
fn view_forward_yaw_zero_is_plus_x() {
    let f = view_forward(0.0);
    assert!((f.x - 1.0).abs() < 0.001);
    assert!(f.y.abs() < 0.001);
}

#[test]
fn view_forward_yaw_90_is_plus_y() {
    let f = view_forward(90.0);
    assert!(f.x.abs() < 0.001);
    assert!((f.y - 1.0).abs() < 0.001);
}

#[test]
fn view_right_yaw_zero_is_minus_y_strafe() {
    // Q2 right = (sin yaw, -cos yaw) at yaw=0 → (0, -1).
    // "right" strafe in Q2 is -Y at yaw=0.
    let r = view_right(0.0);
    assert!(r.x.abs() < 0.001);
    assert!((r.y + 1.0).abs() < 0.001);
}
