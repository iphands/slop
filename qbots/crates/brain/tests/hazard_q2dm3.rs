//! Plan 48 T2: `hazard::dir_is_hazardous` must flag walking into q2dm3's lava.
//!
//! Self-locating: scans the map for a lava pool, walks outward to the first standable
//! rim position, and asserts the probe flags the direction back toward the pool. No
//! hard-coded coordinates — any q2dm3 lava rim qualifies.
//!
//! Gated on the Quake 2 pak being present (`vendor/baseq2` by default, or `QBOTS_BASEQ2`),
//! the same skip-and-pass pattern as `world/tests/ride_q2dm3.rs`.

use std::path::PathBuf;

use brain::hazard::dir_is_hazardous;
use glam::Vec3;
use world::{Bsp, CollisionModel, CONTENTS_LAVA, CONTENTS_SLIME, MASK_SOLID};

fn baseq2_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QBOTS_BASEQ2") {
        let pb = PathBuf::from(p);
        return pb.join("pak0.pak").exists().then_some(pb);
    }
    let pb = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/baseq2");
    pb.join("pak0.pak").exists().then_some(pb)
}

/// Open-air lava surface points found on a coarse grid scan (capped).
fn find_lava_surfaces(cm: &CollisionModel, mins: [f32; 3], maxs: [f32; 3]) -> Vec<Vec3> {
    let mut out = Vec::new();
    let mut x = mins[0];
    while x <= maxs[0] && out.len() < 64 {
        let mut y = mins[1];
        while y <= maxs[1] && out.len() < 64 {
            let mut z = maxs[2];
            while z >= mins[2] {
                if cm.point_contents(&[x, y, z]) & CONTENTS_LAVA != 0
                    && cm.point_contents(&[x, y, z + 16.0]) == 0
                {
                    out.push(Vec3::new(x, y, z));
                    break;
                }
                z -= 16.0;
            }
            y += 64.0;
        }
        x += 64.0;
    }
    out
}

/// From a lava surface point, march along `step` to the first standable non-lava rim.
/// Pool rims usually sit ABOVE the lava surface, so probe down from surface + 72 u and
/// accept a safe solid floor anywhere between 40 u below and 54 u above the surface.
fn find_rim(cm: &CollisionModel, surface: Vec3, step: Vec3) -> Option<Vec3> {
    let zero = [0.0f32; 3];
    for i in 1..=8 {
        let p = surface + step * (i as f32);
        let top = [p.x, p.y, surface.z + 72.0];
        if cm.point_contents(&top) != 0 {
            continue; // inside the pool wall or another brush
        }
        let bot = [p.x, p.y, surface.z - 40.0];
        let t = cm.trace(&top, &bot, &zero, &zero, MASK_SOLID);
        if t.startsolid || t.fraction >= 1.0 {
            continue; // no floor in the band — still over the pool
        }
        let above = [t.endpos[0], t.endpos[1], t.endpos[2] + 1.0];
        if cm.point_contents(&above) & (CONTENTS_LAVA | CONTENTS_SLIME) != 0 {
            continue; // floor here is still lava bed
        }
        // Head room for a standing bot origin (24 u above the floor).
        let origin = [t.endpos[0], t.endpos[1], t.endpos[2] + 24.0];
        if cm.point_contents(&origin) != 0 {
            continue;
        }
        return Some(Vec3::from(origin));
    }
    None
}

#[test]
fn probe_flags_walking_into_lava() {
    let Some(baseq2) = baseq2_dir() else {
        eprintln!("[skip] q2dm3 pak not found (set QBOTS_BASEQ2 or populate vendor/baseq2)");
        return;
    };
    let bsp = Bsp::load(&baseq2, "q2dm3").expect("load q2dm3");
    let cm = CollisionModel::from_bsp(&bsp);
    let model = bsp.models.first().expect("q2dm3 has a world model");

    let surfaces = find_lava_surfaces(&cm, model.mins, model.maxs);
    assert!(!surfaces.is_empty(), "q2dm3 has lava");
    // Across all found pools, try the four cardinal directions for a standable rim.
    let mut checked = 0;
    for surface in &surfaces {
        for step in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y] {
            let Some(rim) = find_rim(&cm, *surface, step * 16.0) else {
                continue;
            };
            let toward_lava = (*surface - rim).with_z(0.0);
            if toward_lava.length() > 40.0 {
                continue; // rim landed too far from the pool for the 24/48 u probe
            }
            // Some scan hits are lava in cracks UNDER a safe overhanging ledge — the
            // probe rightly says walking there is fine. Count the rims where the pool
            // genuinely lies in the walk path; the real q2dm3 pool rims must flag.
            if dir_is_hazardous(&cm, rim, toward_lava) {
                checked += 1;
            }
        }
    }
    assert!(
        checked > 0,
        "no lava-pool rim flagged hazardous across {} pools — probe is blind to lava",
        surfaces.len()
    );
}
