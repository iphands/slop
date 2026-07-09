//! Plan 42 T6 integration proof: q2dm3's quad and loop-train railgun join the reachable
//! nav graph via `EdgeKind::Ride` edges (`func_train` ride + `func_plat`/ladder vertical
//! rides). Before ride edges, both items sat in isolated components with `path == None`;
//! after Plan 42 (cache v16→v18) an A* route from a DM spawn to each exists and traverses
//! at least one `Ride` edge (the only way onto those platforms).
//!
//! Gated on the Quake 2 pak being present (`vendor/baseq2` by default, or `QBOTS_BASEQ2`).
//! When the pak is absent — CI without game data — the test logs a skip and passes, the
//! same pattern `water_q2dm1.rs` uses for pak-dependent tests.

use std::path::PathBuf;

use world::{generate_map_nav, ELEVATOR_PENALTY, GRID_SPACING};

/// q2dm3 `item_quad` entity origin (upper level, reached by riding `func_train *10`).
const QUAD: [f32; 3] = [192.0, 320.0, 216.0];
/// q2dm3 loop-train `weapon_railgun` instance 1 (reached by riding `*3`/`*4` + the lift).
const RAILGUN_LOOP: [f32; 3] = [768.0, 816.0, 208.0];

fn baseq2_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QBOTS_BASEQ2") {
        let pb = PathBuf::from(p);
        return pb.join("pak0.pak").exists().then_some(pb);
    }
    // Default: <repo>/vendor/baseq2 (this crate lives at <repo>/crates/world).
    let pb = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/baseq2");
    pb.join("pak0.pak").exists().then_some(pb)
}

/// Assert at least one DM spawn reaches `goal_pos` and the route uses ≥1 `Ride` edge.
fn assert_reachable_via_ride(built: &world::MapNavBuild, goal_pos: &[f32; 3], label: &str) {
    let g = &built.graph;
    let goal = g
        .nearest(goal_pos)
        .unwrap_or_else(|| panic!("nearest node to {label}"));

    let mut any_reached = false;
    for sp in &built.spawn_origins {
        let start = match g.nearest(sp) {
            Some(n) => n,
            None => continue,
        };
        if let Some(path) = g.path(start, goal) {
            let rides = path
                .windows(2)
                .filter(|w| g.is_ride_edge(w[0], w[1]))
                .count();
            assert!(
                rides > 0,
                "{label} route must traverse a ride edge (train/lift/ladder is the only way in)"
            );
            any_reached = true;
        }
    }
    assert!(
        any_reached,
        "no spawn reached the q2dm3 {label} — ride-edge nav regression"
    );
}

#[test]
fn q2dm3_quad_and_railgun_reachable_by_ride() {
    let Some(baseq2) = baseq2_dir() else {
        eprintln!("[skip] q2dm3 pak not found (set QBOTS_BASEQ2 or populate vendor/baseq2)");
        return;
    };

    // Lift-penalty 0 so A* is free to route via the ride edges (the intended path), matching
    // the Plan 43 validation commands and the eventual Plan 31 penalty removal.
    let built =
        generate_map_nav(&baseq2, "q2dm3", 0.0, GRID_SPACING).expect("build q2dm3 nav graph");
    let _ = ELEVATOR_PENALTY; // documents the penalty this test intentionally bypasses.

    assert_reachable_via_ride(&built, &QUAD, "quad");
    assert_reachable_via_ride(&built, &RAILGUN_LOOP, "loop-train railgun");
}
