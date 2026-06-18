//! Unit tests for `brain::steer` and `brain::nav` — Plan 12 T1, T2, T3.

use brain::nav::{NavGoal, NavigationDriver, LOOKAHEAD};
use brain::steer::{
    move_from_world_dir, view_forward, view_right, Steering, ARRIVE_RADIUS, YAW_SPEED_BASE,
};

use glam::Vec3;
use std::sync::Arc;
use world::NavGraph;

// ── T1: change_yaw ────────────────────────────────────────────────────────────

#[test]
fn change_yaw_shortest_arc_positive() {
    // From 0°, ideal = +179°. Shortest arc = +179 (not -181).
    let mut s = Steering::new(1.0); // combat_skill=1 → 720°/s base
    let dt = 1.0; // 1 s → max_step = YAW_SPEED_BASE
    let result = s.change_yaw(179.0, dt);
    // At 720 dps and 1 s dt, full 179° is reachable in one step.
    assert!((result - 179.0).abs() < 0.01, "expected ~179, got {result}");
}

#[test]
fn change_yaw_shortest_arc_negative() {
    // From 0°, ideal = -179°. Shortest arc = -179 (not +181).
    let mut s = Steering::new(1.0);
    let result = s.change_yaw(-179.0, 1.0);
    assert!(
        (result - (-179.0)).abs() < 0.01,
        "expected ~-179, got {result}"
    );
}

#[test]
fn change_yaw_shortest_arc_chooses_small_side() {
    // From 0°, ideal = +181°. The shortest arc is −179° (go left), not +181°.
    let mut s = Steering::new(1.0);
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
    let mut s = Steering::new(1.0); // combat=1 → yaw_speed=720
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
    let mut s = Steering::new(1.0);
    s.set_view_yaw(89.5);
    let result = s.change_yaw(90.0, 0.1); // only 0.5° remaining, max step = 72°
    assert!((result - 90.0).abs() < 0.01, "overshot: got {result}");
}

#[test]
fn change_yaw_skill_scaling_monotonic() {
    // Higher combat skill → faster turn per dt. Compare magnitudes (direction can vary).
    let dt = 0.1;
    let ideal = 90.0; // unambiguous +90° arc
                      // Combat skills in [1.0, 5.0] — each level adds YAW_SPEED_PER_LEVEL.
    let deltas: Vec<f32> = [1.0f32, 2.0, 3.0, 4.0, 5.0]
        .iter()
        .map(|&skill| {
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
    let mut s = Steering::new(5.0); // combat=5 → fast turn
                                    // Do many small steps pushing past 180°.
    for _ in 0..20 {
        s.change_yaw(360.0, 0.1);
    }
    let yaw = s.view_yaw();
    assert!((-180.0..180.0).contains(&yaw), "yaw out of range: {yaw}");
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

// ── T3: arrive_scale ─────────────────────────────────────────────────────────

#[test]
fn arrive_scale_outside_radius_is_one() {
    assert_eq!(Steering::arrive_scale(ARRIVE_RADIUS + 1.0), 1.0);
    assert_eq!(Steering::arrive_scale(1000.0), 1.0);
}

#[test]
fn arrive_scale_at_half_radius_is_half() {
    let scale = Steering::arrive_scale(ARRIVE_RADIUS / 2.0);
    assert!((scale - 0.5).abs() < 0.01, "expected 0.5, got {scale}");
}

#[test]
fn arrive_scale_very_close_clamps_to_min() {
    // Close to 0 distance → clamps to ARRIVE_MIN (0.25), not 0.
    let scale = Steering::arrive_scale(1.0);
    assert!(
        scale >= 0.25,
        "arrive_scale should clamp to at least 0.25, got {scale}"
    );
    assert!(
        scale < 0.1 + 0.25,
        "should be close to minimum at 1 unit distance, got {scale}"
    );
}

// ── T3: pursue_target ─────────────────────────────────────────────────────────

/// Build a linear nav graph: nodes at 0, 100, 200, 300 on the X axis.
fn linear_graph() -> Arc<NavGraph> {
    Arc::new(NavGraph::from_raw(
        vec![
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [200.0, 0.0, 0.0],
            [300.0, 0.0, 0.0],
        ],
        vec![
            vec![(1, 100.0)],
            vec![(0, 100.0), (2, 100.0)],
            vec![(1, 100.0), (3, 100.0)],
            vec![(2, 100.0)],
        ],
    ))
}

#[test]
fn pursue_target_returns_lookahead_point_on_path() {
    let g = linear_graph();
    let mut nav = NavigationDriver::new(Arc::clone(&g));
    // Start at node 0 (0,0,0), drive to node 3 (300,0,0).
    nav.set_goal(NavGoal::Waypoint(3), Vec3::ZERO);

    let from = Vec3::new(0.0, 0.0, 0.0);
    let target = nav
        .pursue_target(from)
        .expect("should have a pursue target");
    // LOOKAHEAD = 96. Path: 0→100→200→300. First segment is 100 units long.
    // At 96 units along the 0→100 segment, we get (96, 0, 0).
    assert!(
        (target.x - LOOKAHEAD).abs() < 1.0,
        "expected x≈{LOOKAHEAD}, got {}",
        target.x
    );
    assert!(target.y.abs() < 0.01);
}

#[test]
fn pursue_target_crosses_segment_boundary() {
    let g = linear_graph();
    let mut nav = NavigationDriver::new(Arc::clone(&g));
    nav.set_goal(NavGoal::Waypoint(3), Vec3::ZERO);

    // Advance: bot is now at node 1 (100,0,0), current waypoint = 1.
    nav.update(Vec3::new(0.0, 0.0, 0.0), None); // advance to node 1? No: 100u away, not reached yet
                                                // Force the current_waypoint to node 2 by simulating arrival at node 1.
    let from = Vec3::new(90.0, 0.0, 0.0); // close to node 1 but not arrived

    // Now: current_waypoint = 1, LOOKAHEAD = 96 from (90, 0, 0).
    // Distance to node 1 (100, 0, 0) = 10. Remaining = 86. Then to node 2 = 100u.
    // Total seg: 10 + 86 → target at (186, 0, 0).
    let target = nav.pursue_target(from).expect("should have target");
    assert!(
        (target.x - (90.0 + LOOKAHEAD)).abs() < 2.0,
        "expected x≈{}, got {}",
        90.0 + LOOKAHEAD,
        target.x
    );
}

#[test]
fn pursue_target_returns_final_goal_when_path_shorter_than_lookahead() {
    // Graph with just 2 nodes, 10 units apart — shorter than LOOKAHEAD.
    let g = Arc::new(NavGraph::from_raw(
        vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
        vec![vec![(1, 10.0)], vec![(0, 10.0)]],
    ));
    let mut nav = NavigationDriver::new(Arc::clone(&g));
    nav.set_goal(NavGoal::Waypoint(1), Vec3::ZERO);

    let target = nav.pursue_target(Vec3::ZERO).expect("target");
    // Path shorter than LOOKAHEAD → returns the final node at (10, 0, 0).
    assert!(
        (target.x - 10.0).abs() < 0.1,
        "should return final node x=10, got {}",
        target.x
    );
}

// ── T3: orbit-timeout node advance ───────────────────────────────────────────

#[test]
fn orbit_timeout_advances_after_n_ticks_near_waypoint() {
    // A two-node graph. Bot starts right at the start node (which it can't "reach"
    // by the strict Z-aware gate since it's at the same position and update skips
    // advancing for the start node). We'll use a 3-node graph and hover near node 1.
    let g = Arc::new(NavGraph::from_raw(
        vec![
            [0.0, 0.0, 0.0],
            [50.0, 0.0, 0.0], // node 1 — orbit around this
            [200.0, 0.0, 0.0],
        ],
        vec![
            vec![(1, 50.0)],
            vec![(0, 50.0), (2, 150.0)],
            vec![(1, 150.0)],
        ],
    ));
    let mut nav = NavigationDriver::new(Arc::clone(&g));
    nav.set_goal(NavGoal::Waypoint(2), Vec3::ZERO);

    // Hover inside ORBIT_RADIUS=48 of node 1 (50,0,0) but outside the Z-aware reach
    // gate (WP_REACH_HORIZ=32): horiz to node 1 = 40 > 32 → not reached, < 48 → orbiting.
    let hover_pos = Vec3::new(10.0, 0.0, 0.0); // 40u horizontal from node 1 at (50,0,0)

    // The nav driver starts at node 0's path; first advance to node 1:
    // Position at (0,0,0) is the start — set_goal commits to node 1 as current_waypoint.
    assert_eq!(nav.current_waypoint(), Some(1));

    // Hover near node 1 for ORBIT_FRAMES ticks — should force-advance to node 2.
    let orbit_frames = brain::nav::ORBIT_FRAMES;
    let mut advanced = false;
    for tick in 0..=orbit_frames {
        nav.update(hover_pos, None);
        if nav.current_waypoint() == Some(2) {
            advanced = true;
            assert!(
                tick >= orbit_frames - 1,
                "advanced too early at tick {tick}"
            );
            break;
        }
    }
    assert!(
        advanced,
        "orbit-timeout should have force-advanced past node 1"
    );
}
