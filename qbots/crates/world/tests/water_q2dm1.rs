//! Plan 39 integration proof: the q2dm1 railgun room is reachable in the A* nav graph
//! only by swimming. Before water nav, `path(spawn → railgun)` was `None` (the railgun
//! floor was an isolated component); with swim nodes/edges it must be `Some`.
//!
//! Gated on the Quake 2 pak being present (`vendor/baseq2` by default, or `QBOTS_BASEQ2`).
//! When the pak is absent — CI without game data — the test logs a skip and passes, the
//! same pattern `navinspect QBOTS_LIVE` uses for pak-dependent diagnostics.

use std::path::PathBuf;

use world::{generate_map_nav, GRID_SPACING};

/// q2dm1 `weapon_railgun` entity origin (from the BSP entity lump).
const RAILGUN: [f32; 3] = [240.0, -384.0, 464.0];
/// Two DM spawns the Plan 39 baseline probed — both returned NO PATH before the fix.
const SPAWNS: [[f32; 3]; 2] = [[1488.0, -48.0, 664.0], [544.0, 352.0, 482.0]];

fn baseq2_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QBOTS_BASEQ2") {
        let pb = PathBuf::from(p);
        return pb.join("pak0.pak").exists().then_some(pb);
    }
    // Default: <repo>/vendor/baseq2 (this crate lives at <repo>/crates/world).
    let pb = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/baseq2");
    pb.join("pak0.pak").exists().then_some(pb)
}

#[test]
fn q2dm1_railgun_reachable_by_swim() {
    let Some(baseq2) = baseq2_dir() else {
        eprintln!("[skip] q2dm1 pak not found (set QBOTS_BASEQ2 or populate vendor/baseq2)");
        return;
    };

    let built = generate_map_nav(&baseq2, "q2dm1", GRID_SPACING).expect("build q2dm1 nav graph");
    let g = &built.graph;

    let goal = g.nearest(&RAILGUN).expect("nearest node to railgun");

    // At least one DM spawn must now reach the railgun (it was NONE for all before Plan 39).
    let mut any_reached = false;
    for sp in SPAWNS {
        let start = g.nearest(&sp).expect("nearest node to spawn");
        if let Some(path) = g.path(start, goal) {
            let swims = path
                .windows(2)
                .filter(|w| g.is_swim_edge(w[0], w[1]))
                .count();
            assert!(
                swims > 0,
                "railgun route must traverse swim edges (the only way in)"
            );
            any_reached = true;
        }
    }
    assert!(
        any_reached,
        "no spawn reached the q2dm1 railgun — water nav regression"
    );
}
