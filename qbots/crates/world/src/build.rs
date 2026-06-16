//! Consolidated BSP → collision model → nav graph build pipeline (Plan 18 T1).
//!
//! Before this, `scenario.rs` and `supervisor.rs` each duplicated the
//! `Bsp::load` → `CollisionModel::from_bsp` → `NavGraph::generate` → `seed_spawns` →
//! `detect_jump_edges` → `spawns_in_largest_component` sequence with independently
//! drifting spacing literals. One function, one set of constants.

use std::path::Path;
use std::sync::Arc;

use crate::bsp::Bsp;
use crate::collision::CollisionModel;
use crate::navgraph::NavGraph;

/// Grid spacing (units) for `NavGraph::generate`'s waypoint sampling.
pub const GRID_SPACING: f32 = 24.0;
/// Max probe distance (units) for `NavGraph::detect_jump_edges`'s ledge-drop search.
pub const JUMP_SPACING: f32 = 64.0;

/// Everything a caller needs after building a map's nav graph: the parsed BSP
/// (for spawn points / entity lookups), the collision model (for traces/LOS),
/// the finished graph, and the seeding/connectivity counters for logging.
pub struct MapNavBuild {
    pub bsp: Bsp,
    pub cm: Arc<CollisionModel>,
    pub graph: NavGraph,
    pub spawn_origins: Vec<[f32; 3]>,
    pub seeded: usize,
    pub added_jumps: usize,
    pub in_largest: usize,
    pub total_spawns: usize,
    /// Nodes in the largest connected component — used for bot roaming.
    pub largest: Vec<usize>,
}

/// Run the full build pipeline for `map` under `baseq2`: load the BSP, build the
/// collision model, sample the nav graph, seed DM spawns as nodes, and detect
/// ledge-drop jump edges. Returns `Err` only on load/parse failure or a BSP with
/// no models — never partial output.
pub fn generate_map_nav(baseq2: &Path, map: &str) -> Result<MapNavBuild, String> {
    let bsp = Bsp::load(baseq2, map)?;
    let cm = Arc::new(CollisionModel::from_bsp(&bsp));

    let bounds = {
        let model = bsp
            .models
            .first()
            .ok_or_else(|| format!("BSP for '{map}' has no models"))?;
        (model.mins, model.maxs)
    };

    let spawn_origins: Vec<[f32; 3]> = bsp.spawn_points().iter().map(|s| s.origin).collect();

    let mut graph = NavGraph::generate(&cm, bounds, GRID_SPACING);
    let seeded = graph.seed_spawns(&cm, &spawn_origins);
    let added_jumps = graph.detect_jump_edges(&cm, JUMP_SPACING);
    let (in_largest, total_spawns) = graph.spawns_in_largest_component(&spawn_origins);
    let largest = graph.components().into_iter().next().unwrap_or_default();

    Ok(MapNavBuild {
        bsp,
        cm,
        graph,
        spawn_origins,
        seeded,
        added_jumps,
        in_largest,
        total_spawns,
        largest,
    })
}
