//! Consolidated BSP → collision model → nav graph build pipeline (Plan 18 T1).
//!
//! Before this, `scenario.rs` and `supervisor.rs` each duplicated the
//! `Bsp::load` → `CollisionModel::from_bsp` → `NavGraph::generate` → `seed_spawns` →
//! `detect_jump_edges` → `spawns_in_largest_component` sequence with independently
//! drifting spacing literals. One function, one set of constants.

use std::path::Path;
use std::sync::Arc;

use crate::bsp::{Bsp, BspEntity};
use crate::collision::CollisionModel;
use crate::mapcache::{self, Fingerprint};
use crate::navgraph::NavGraph;

/// Grid spacing (units) for `NavGraph::generate`'s waypoint sampling.
pub const GRID_SPACING: f32 = 24.0;
/// Max probe distance (units) for `NavGraph::detect_jump_edges`'s ledge-drop search.
pub const JUMP_SPACING: f32 = 64.0;
/// Max horizontal distance (units) for `NavGraph::bridge_components` to stitch two
/// disconnected-but-walkable fragments. Must be large enough to span the widest
/// inter-component staircase gap in q2dm* maps. q2dm1 has at least one winding
/// staircase whose endpoints land in different grid columns ~160–200u apart —
/// BRIDGE_HDIST=128 misses that gap, leaving 2 major components disconnected.
/// 256u bridges the real winding staircase while the floor-existence check inside
/// `walkable_stair` still rejects false long-range cross-floor shortcuts.
/// Must stay in the cache fingerprint so changing it auto-invalidates stale caches.
pub const BRIDGE_HDIST: f32 = 256.0;

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

/// Like `generate_map_nav` but checks `cache_dir/<map>.qnav` first. On a cache
/// hit, graph generation is skipped entirely — BSP load + CM build still happen
/// (needed for collision traces at runtime). On a miss or a stale fingerprint,
/// generates live, saves the cache, and logs a one-liner either way.
///
/// `cache_dir` is typically `./data/mapcache`; pass `None` to skip cache I/O.
pub fn cached_map_nav(
    baseq2: &Path,
    map: &str,
    cache_dir: Option<&Path>,
) -> Result<MapNavBuild, String> {
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
    let fp = Fingerprint::from_bsp(&bsp);

    // Try the disk cache if a directory was provided.
    if let Some(dir) = cache_dir {
        let cache_path = dir.join(format!("{map}.qnav"));
        if let Some(cached_graph) = mapcache::load(&cache_path, &fp) {
            let seeded = 0; // graph already has spawn nodes baked in from the prior run
            let added_jumps = 0;
            let (in_largest, total_spawns) =
                cached_graph.spawns_in_largest_component(&spawn_origins);
            let largest = cached_graph.largest_spawn_component(&spawn_origins);
            tracing::info!(
                map,
                nodes = cached_graph.node_count(),
                edges = cached_graph.edge_count(),
                "nav graph: loaded from cache"
            );
            return Ok(MapNavBuild {
                bsp,
                cm,
                graph: cached_graph,
                spawn_origins,
                seeded,
                added_jumps,
                in_largest,
                total_spawns,
                largest,
            });
        }
        tracing::info!(
            map,
            "nav graph: no fresh cache — run 'qbots generate-map-cache --map {map}' to speed up future runs"
        );
    }

    // Cache miss (or no cache dir): generate live.
    let mut graph = NavGraph::generate(&cm, bounds, GRID_SPACING);
    let seeded = graph.seed_spawns(&cm, &spawn_origins);
    add_elevator_edges(&mut graph, &cm, &bsp);
    graph.bridge_components(&cm, BRIDGE_HDIST);
    let added_jumps = graph.detect_jump_edges(&cm, JUMP_SPACING);
    let (in_largest, total_spawns) = graph.spawns_in_largest_component(&spawn_origins);
    let largest = graph.largest_spawn_component(&spawn_origins);

    // Save the cache for next time.
    if let Some(dir) = cache_dir {
        let cache_path = dir.join(format!("{map}.qnav"));
        if let Err(e) = mapcache::save(&cache_path, &graph, &fp) {
            tracing::warn!(map, "nav graph cache save failed: {e}");
        }
    }

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
    add_elevator_edges(&mut graph, &cm, &bsp);
    graph.bridge_components(&cm, BRIDGE_HDIST);
    let added_jumps = graph.detect_jump_edges(&cm, JUMP_SPACING);
    let (in_largest, total_spawns) = graph.spawns_in_largest_component(&spawn_origins);
    let largest = graph.largest_spawn_component(&spawn_origins);

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

/// Parse `func_plat` (elevator) entities from the BSP and add nav nodes at the
/// platform's top and bottom travel positions. Each elevator gets two nodes (top
/// and bottom of its travel path) wired together and connected to nearby walkable
/// nodes so the pathfinder can plan "ride the elevator" routes.
///
/// Q2DM maps that have no walkable staircase between two floor levels use
/// `func_plat` as the ONLY vertical connector. Without this, the lower indoor
/// areas are permanently disconnected from the upper outdoor areas.
///
/// Returns the number of elevator edge-pairs added (each pair = 1 top↔bottom edge
/// plus however many edges connected those nodes to their local nav graphs).
pub fn add_elevator_edges(graph: &mut NavGraph, cm: &CollisionModel, bsp: &Bsp) -> usize {
    let mut added = 0;
    // `func_plat`: auto-lowering platform, rests at top, travels down.
    for entity in bsp.find_class("func_plat") {
        if let Some(n) = try_add_plat(graph, cm, bsp, entity) {
            added += n;
        }
    }
    // `func_door` used as a vertical lift (angle -1 = up, -2 = down). Geometrically
    // identical to a plat for nav: the brush's top surface occupies two z-levels.
    // Horizontal doors are skipped — their doorway floor lives in world model 0 and
    // is already sampled, so they never fragment the graph.
    for entity in bsp.find_class("func_door") {
        if let Some(n) = try_add_vertical_door(graph, cm, bsp, entity) {
            added += n;
        }
    }
    added
}

/// Resolve an entity's `"model" "*N"` field to its inline BSP model.
fn entity_model<'a>(bsp: &'a Bsp, entity: &BspEntity) -> Option<&'a crate::bsp::Model> {
    let model_idx: usize = entity
        .fields
        .get("model")
        .and_then(|s| s.strip_prefix('*'))
        .and_then(|s| s.trim().parse().ok())?;
    bsp.models.get(model_idx)
}

/// Add a two-level vertical lift between top-surface world z-values `z_hi` and `z_lo`
/// at the brush's XY center: a nav node per level, an edge for the ride itself, and
/// trace-checked edges from each node to nearby walkable floor nodes. Returns the
/// number of edges added (≥1 for the ride). `label` is for the debug log.
fn add_lift(
    graph: &mut NavGraph,
    cm: &CollisionModel,
    model: &crate::bsp::Model,
    z_hi: f32,
    z_lo: f32,
    label: &str,
) -> usize {
    let cx = (model.mins[0] + model.maxs[0]) / 2.0;
    let cy = (model.mins[1] + model.maxs[1]) / 2.0;
    // Nav node = player origin = floor_surface_z + 24 (hull_mins.z = -24).
    let top_node = [cx, cy, z_hi + 24.0];
    let bot_node = [cx, cy, z_lo + 24.0];

    tracing::debug!(cx, cy, z_hi, z_lo, travel = z_hi - z_lo, "{label}");

    let top_idx = graph.add_node(top_node);
    let bot_idx = graph.add_node(bot_node);
    graph.add_edge(top_idx, bot_idx, (z_hi - z_lo).abs());
    let mut n = 1;
    // Generous radius (256 u): the platform may land away from sampled grid cells.
    n += graph.connect_node_to_nearby(cm, top_idx, 256.0);
    n += graph.connect_node_to_nearby(cm, bot_idx, 256.0);
    n
}

fn try_add_plat(
    graph: &mut NavGraph,
    cm: &CollisionModel,
    bsp: &Bsp,
    entity: &BspEntity,
) -> Option<usize> {
    let model = entity_model(bsp, entity)?;

    // Travel distance: the platform rests at the TOP in the BSP and travels DOWN.
    // `lip` is how much it protrudes at the bottom (default 8). If `height` is set
    // in the entity it overrides the default (model Z extent - lip).
    // (`g_func.c:SP_func_plat`, lines 822-837.)
    let lip: f32 = entity
        .fields
        .get("lip")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(8.0_f32);
    let travel: f32 = entity
        .fields
        .get("height")
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|&h| h > 0.0)
        .unwrap_or_else(|| (model.maxs[2] - model.mins[2]) - lip);

    if travel <= 0.0 {
        return None;
    }
    Some(add_lift(
        graph,
        cm,
        model,
        model.maxs[2],
        model.maxs[2] - travel,
        "func_plat elevator bridge",
    ))
}

fn try_add_vertical_door(
    graph: &mut NavGraph,
    cm: &CollisionModel,
    bsp: &Bsp,
    entity: &BspEntity,
) -> Option<usize> {
    // `angle` is the special move direction: -1 = up, -2 = down (`G_SetMovedir`,
    // g_utils.c:381). Anything else is a horizontal door — not a lift; skip it.
    let angle = entity
        .fields
        .get("angle")
        .and_then(|s| s.trim().parse::<f32>().ok());
    let dir = match angle {
        Some(a) if (a - -1.0).abs() < 0.5 => 1.0,  // opens UP
        Some(a) if (a - -2.0).abs() < 0.5 => -1.0, // opens DOWN
        _ => return None,
    };

    let model = entity_model(bsp, entity)?;

    // Travel = size_z - lip (lip default 8 for func_door, `g_func.c:1795-1813`).
    let lip: f32 = entity
        .fields
        .get("lip")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(8.0_f32);
    let travel = (model.maxs[2] - model.mins[2]) - lip;
    if travel <= 0.0 {
        return None;
    }

    // The brush's spawn bounds sit at one rest position; the top surface also occupies
    // `maxs[2] + dir*travel`. The two physical levels are independent of which one is
    // "open" (DOOR_START_OPEN only swaps the rest pose), so take the hi/lo of the pair.
    let other = model.maxs[2] + dir * travel;
    let z_hi = model.maxs[2].max(other);
    let z_lo = model.maxs[2].min(other);
    Some(add_lift(
        graph,
        cm,
        model,
        z_hi,
        z_lo,
        "func_door lift bridge",
    ))
}

/// Returns `Ok(())` if every DM spawn point is reachable from the largest
/// connected component of the nav graph. On failure returns a detailed
/// multi-line diagnostic string intended as the body of a fatal error log.
///
/// All Q2 deathmatch maps guarantee mutual spawn reachability by design.
/// A failure here is **always** a bug in BSP parsing, collision model, or
/// nav graph generation — never a legitimate map property. Callers must treat
/// the `Err` case as fatal: do not cache a broken graph, do not run bots on it.
pub fn check_spawn_connectivity(built: &MapNavBuild) -> Result<(), String> {
    if built.in_largest == built.total_spawns {
        return Ok(());
    }

    let comps = built.graph.components();
    let largest_set: std::collections::HashSet<usize> = comps
        .first()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();

    let mut msg = format!(
        "NAV CONNECTIVITY BUG: only {}/{} spawn points are in the largest component.\n\
         All Q2 deathmatch maps guarantee every spawn is mutually reachable by design.\n\
         This is a bug in BSP parsing, collision model, or nav graph generation.\n\
         See context/pitfalls.md for known issues.\n\
         nodes={} edges={} total_components={} largest_component_size={}",
        built.in_largest,
        built.total_spawns,
        built.graph.node_count(),
        built.graph.edge_count(),
        comps.len(),
        comps.first().map_or(0, |c| c.len()),
    );

    msg.push_str("\n--- spawn diagnostics ---");
    for (i, sp) in built.spawn_origins.iter().enumerate() {
        let nearest = built.graph.nearest(sp);
        let in_lg = nearest.is_some_and(|n| largest_set.contains(&n));
        let comp_idx = nearest
            .and_then(|n| comps.iter().position(|c| c.contains(&n)))
            .unwrap_or(999);
        msg.push_str(&format!(
            "\n  spawn[{i}] ({:.0},{:.0},{:.0}) -> node={:?} component={} {}",
            sp[0],
            sp[1],
            sp[2],
            nearest,
            comp_idx,
            if in_lg {
                "[ok]"
            } else {
                "[BUG: not in largest]"
            },
        ));
    }

    msg.push_str("\n--- component sizes (largest first) ---");
    for (i, c) in comps.iter().enumerate().take(10) {
        msg.push_str(&format!("\n  component[{i}]: {} nodes", c.len()));
    }
    if comps.len() > 10 {
        msg.push_str(&format!(
            "\n  ... {} more components omitted",
            comps.len() - 10
        ));
    }

    Err(msg)
}
