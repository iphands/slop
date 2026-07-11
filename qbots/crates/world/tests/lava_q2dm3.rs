//! Plan 48 T1 regression: `segment_has_floor` must reject straight lines over lava.
//!
//! Pre-fix, the floor probe traced down with `MASK_SOLID` only, so the solid *bed* of a
//! shallow lava pool registered as walkable floor and the corner-cut guard
//! (`pursue_target_safe`) approved shortcuts straight across q2dm3's lava. The test
//! self-locates a lava-covered floor column (no hard-coded coordinates — any q2dm3 lava
//! qualifies) and asserts the probe rejects a segment crossing it, while a segment on a
//! DM spawn floor still passes.
//!
//! Gated on the Quake 2 pak being present (`vendor/baseq2` by default, or `QBOTS_BASEQ2`).
//! When the pak is absent — CI without game data — the test logs a skip and passes, the
//! same pattern `ride_q2dm3.rs` uses for pak-dependent tests.

use std::path::PathBuf;

use world::navgraph::segment_has_floor;
use world::{Bsp, CollisionModel, CONTENTS_LAVA, MASK_SOLID};

fn baseq2_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QBOTS_BASEQ2") {
        let pb = PathBuf::from(p);
        return pb.join("pak0.pak").exists().then_some(pb);
    }
    let pb = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/baseq2");
    pb.join("pak0.pak").exists().then_some(pb)
}

/// Scan the map on a coarse grid for a lava volume: walk each column's contents in 16 u
/// steps (interior floors hide lava from a single top-down trace). Returns the lava
/// *surface* position (topmost in-lava point of the first span found) — but only for
/// pools with air directly above (a bot can actually walk into them).
fn find_lava_surface(cm: &CollisionModel, mins: [f32; 3], maxs: [f32; 3]) -> Option<[f32; 3]> {
    let mut x = mins[0];
    while x <= maxs[0] {
        let mut y = mins[1];
        while y <= maxs[1] {
            let mut z = maxs[2];
            while z >= mins[2] {
                if cm.point_contents(&[x, y, z]) & CONTENTS_LAVA != 0
                    && cm.point_contents(&[x, y, z + 16.0]) == 0
                {
                    return Some([x, y, z]);
                }
                z -= 16.0;
            }
            y += 64.0;
        }
        x += 64.0;
    }
    None
}

#[test]
fn segment_has_floor_rejects_lava_crossings() {
    let Some(baseq2) = baseq2_dir() else {
        eprintln!("[skip] q2dm3 pak not found (set QBOTS_BASEQ2 or populate vendor/baseq2)");
        return;
    };
    let bsp = Bsp::load(&baseq2, "q2dm3").expect("load q2dm3");
    let cm = CollisionModel::from_bsp(&bsp);
    let model = bsp.models.first().expect("q2dm3 has a world model");

    let surface =
        find_lava_surface(&cm, model.mins, model.maxs).expect("q2dm3 has lava-covered floor");
    // Locate the solid bed under the lava, then pick a segment height the OLD probe
    // definitely reached (bed + 88 < bed + FLOOR_PROBE=96): pre-fix the bed counted as
    // floor and this segment passed; post-fix the lava contents/deadly-floor checks
    // reject it. Capped at surface + 16 so shallow pools test the walk-height case.
    let zero = [0.0f32; 3];
    let bed = cm.trace(
        &[surface[0], surface[1], surface[2]],
        &[surface[0], surface[1], surface[2] - 512.0],
        &zero,
        &zero,
        MASK_SOLID,
    );
    assert!(bed.fraction < 1.0, "lava at {surface:?} has a solid bed");
    let seg_z = (surface[2] + 16.0).min(bed.endpos[2] + 88.0);
    let a = [surface[0] - 32.0, surface[1], seg_z];
    let b = [surface[0] + 32.0, surface[1], seg_z];
    assert!(
        !segment_has_floor(&cm, a, b),
        "segment over lava at {surface:?} must NOT count the lava bed as floor"
    );

    // Control: a short segment on a DM spawn floor is still continuous floor.
    let spawn = bsp
        .spawn_points()
        .first()
        .expect("q2dm3 has spawn points")
        .origin;
    let sa = [spawn[0], spawn[1], spawn[2] + 8.0];
    let sb = [spawn[0] + 48.0, spawn[1], spawn[2] + 8.0];
    assert!(
        segment_has_floor(&cm, sa, sb),
        "spawn-floor segment at {spawn:?} must keep passing"
    );
}
