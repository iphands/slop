//! Plan 63 T1 regression: the navmesh build must not offer deadly (lava/slime) floors.
//!
//! Pre-fix, the heightfield's only liquid check was a single `point_contents` probe at
//! player-origin height (floor + 24): a lava pool ≤ 24u deep leaves that point in air, so
//! the span was accepted as walkable and the mesh routed bots straight across lava — the
//! q2dm6 `--navmodes nm,sg` lava-suicide report. Drops (`find_drops`) validated nothing at
//! the landing. Both are the navmesh analogues of the A* bugs fixed in Plans 48/50
//! (`floor_is_deadly` node sampling, `landing_strip_deadly` jump landings) that were never
//! ported to this builder.
//!
//! Tests self-locate (no hard-coded coordinates) and mirror the live build pipeline
//! (`supervisor::get_or_build_navmesh`): cell 8, drops found pre-erosion, erode(1).
//! The deadly-floor probe is hand-rolled here so the tests stand independent of the
//! (shared) implementation they audit.
//!
//! Gated on the Quake 2 pak being present (`vendor/baseq2` or `QBOTS_BASEQ2`), the same
//! skip pattern as `lava_q2dm3.rs`.

use std::path::PathBuf;

use world::navmesh::{Heightfield, NavMesh, VoxelParams};
use world::{Bsp, CollisionModel, CONTENTS_LAVA, CONTENTS_SLIME, MASK_SOLID};

const MAP: &str = "q2dm6";

fn baseq2_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QBOTS_BASEQ2") {
        let pb = PathBuf::from(p);
        return pb.join("pak0.pak").exists().then_some(pb);
    }
    let pb = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/baseq2");
    pb.join("pak0.pak").exists().then_some(pb)
}

/// Mirror of the live navmesh pipeline (`supervisor.rs get_or_build_navmesh`, erode=1):
/// heightfield → drops on the FULL field → erode → mesh → add_drops.
fn build_mesh(cm: &CollisionModel, bounds: ([f32; 3], [f32; 3])) -> (Heightfield, NavMesh) {
    let params = VoxelParams {
        cell_size: 8.0,
        ..Default::default()
    };
    let mut hf = Heightfield::build(cm, bounds, params);
    let drops = hf.find_drops(cm);
    hf.erode(1);
    let mut mesh = NavMesh::build(&hf, params.walkable_climb, Some(cm));
    mesh.add_drops(&drops);
    (hf, mesh)
}

/// True when the floor under player-origin `oz` at `(x, y)` is a deadly liquid surface:
/// either the origin itself is submerged in lava/slime, or the solid floor the origin
/// stands on is coated by it (shallow pool — the probe at surface + 1 is in liquid).
fn deadly_floor_at(cm: &CollisionModel, x: f32, y: f32, oz: f32) -> bool {
    const DEADLY: i32 = CONTENTS_LAVA | CONTENTS_SLIME;
    if cm.point_contents(&[x, y, oz]) & DEADLY != 0 {
        return true;
    }
    let zero = [0.0f32; 3];
    let down = cm.trace(&[x, y, oz], &[x, y, oz - 96.0], &zero, &zero, MASK_SOLID);
    down.fraction < 1.0 && cm.point_contents(&[x, y, down.endpos[2] + 1.0]) & DEADLY != 0
}

fn load() -> Option<(Bsp, CollisionModel)> {
    let baseq2 = baseq2_dir()?;
    let bsp = Bsp::load(&baseq2, MAP).expect("load q2dm6");
    let cm = CollisionModel::from_bsp(&bsp);
    Some((bsp, cm))
}

#[test]
fn navmesh_offers_no_deadly_floor_spans() {
    let Some((bsp, cm)) = load() else {
        eprintln!("[skip] {MAP} pak not found (set QBOTS_BASEQ2 or populate vendor/baseq2)");
        return;
    };
    let model = bsp.models.first().expect("world model");
    let (hf, mesh) = build_mesh(&cm, (model.mins, model.maxs));

    // Audit every walkable span the mesh was built from (the eroded heightfield): none may
    // stand on a deadly floor. Report a few offenders for debugging.
    let mut deadly = 0usize;
    let mut samples = Vec::new();
    for ci in 0..hf.nx * hf.ny {
        for &oz in &hf.columns[ci] {
            let c = hf.cell_center(ci % hf.nx, ci / hf.nx, oz);
            if deadly_floor_at(&cm, c[0], c[1], c[2]) {
                deadly += 1;
                if samples.len() < 8 {
                    samples.push([c[0] as i32, c[1] as i32, c[2] as i32]);
                }
            }
        }
    }
    assert_eq!(
        deadly, 0,
        "{MAP}: {deadly} walkable navmesh spans stand on lava/slime, e.g. {samples:?}"
    );

    // Post-fix over-pruning guard: the mesh must still connect two DM spawns.
    let spawns = bsp.spawn_points();
    assert!(spawns.len() >= 2, "{MAP} has DM spawns");
    let a = spawns[0].origin;
    let b = spawns[spawns.len() / 2].origin;
    assert!(
        mesh.path(a, b, 16.0).is_some(),
        "{MAP}: navmesh no longer connects spawns {a:?} -> {b:?} (over-pruned?)"
    );
}

#[test]
fn navmesh_drops_never_land_in_lava() {
    let Some((bsp, cm)) = load() else {
        eprintln!("[skip] {MAP} pak not found (set QBOTS_BASEQ2 or populate vendor/baseq2)");
        return;
    };
    let model = bsp.models.first().expect("world model");
    let params = VoxelParams {
        cell_size: 8.0,
        ..Default::default()
    };
    let hf = Heightfield::build(&cm, (model.mins, model.maxs), params);
    let drops = hf.find_drops(&cm);
    assert!(!drops.is_empty(), "{MAP} should have ledge drops");

    // Each drop landing — and the 0..48u momentum-overshoot strip past it, along the
    // drop's horizontal direction — must not be deadly (mirrors `landing_strip_deadly`,
    // the Plan 50 E3 guard on the A* jump/drop builders).
    let mut bad = 0usize;
    let mut samples = Vec::new();
    for (edge, land) in &drops {
        let dx = land[0] - edge[0];
        let dy = land[1] - edge[1];
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        let dir = [dx / len, dy / len];
        let deadly_strip = [0.0f32, 16.0, 32.0, 48.0]
            .iter()
            .any(|&d| deadly_floor_at(&cm, land[0] + dir[0] * d, land[1] + dir[1] * d, land[2]));
        if deadly_strip {
            bad += 1;
            if samples.len() < 8 {
                samples.push([land[0] as i32, land[1] as i32, land[2] as i32]);
            }
        }
    }
    assert_eq!(
        bad,
        0,
        "{MAP}: {bad}/{} navmesh drops land in/skid into lava, e.g. {samples:?}",
        drops.len()
    );
}
