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
/// Max horizontal span (units) for a hull-blocked edge to survive
/// `NavGraph::prune_long_blocked_edges`. Real q2dm1 staircase flights are climbed via
/// short hull-blocked edges (hd ≲ 136); longer hull-blocked edges are false bridges
/// that `walkable_stair` accepted by sampling surfaces across open space — the bot
/// cannot follow them with straight-line steering. 144u keeps real flights, drops the
/// node-10300-style false hubs (hd 187-255). In the cache fingerprint.
pub const PRUNE_MAX_HD: f32 = 144.0;
/// Extra A* cost (units) added to a `func_plat`/lift's vertical ride edge. Riding a Q2
/// elevator can't be modelled by straight-line steering — the bot must wait at the
/// bottom for the plat, ride it, and step off promptly at the top, or it holds the
/// shaft trigger and deadlocks everyone queued below (`g_func.c` Touch_Plat_Center:
/// a player lingering at STATE_TOP resets the go-down timer indefinitely). Until that
/// is modelled, penalise the ride so A* prefers ANY stair/ramp route; the lift is kept
/// (finite cost) only as a last resort for genuinely lift-only spawns. In the cache
/// fingerprint via VERSION bump.
pub const ELEVATOR_PENALTY: f32 = 5000.0;

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

/// Subdirectory name for a given grid spacing, so each spacing keeps its own cache
/// (`data/mapcache/12/q2dm1.qnav`). Lets us flip `--spacing` without clobbering or
/// re-version-bumping. Formats `12.0` as `"12"`, `22.5` as `"22.5"`.
pub fn spacing_subdir(spacing: f32) -> String {
    let s = format!("{spacing}");
    s.trim_end_matches(".0").to_string()
}

/// Like `generate_map_nav` but checks `cache_dir/<spacing>/<map>.qnav` first. On a cache
/// hit, graph generation is skipped entirely — BSP load + CM build still happen
/// (needed for collision traces at runtime). On a miss or a stale fingerprint,
/// generates live, saves the cache, and logs a one-liner either way.
///
/// `cache_dir` is typically `./data/mapcache`; pass `None` to skip cache I/O. `spacing` is
/// the grid spacing the graph is generated at (use `GRID_SPACING` for the default 24u).
pub fn cached_map_nav(
    baseq2: &Path,
    map: &str,
    cache_dir: Option<&Path>,
    lift_penalty: f32,
    spacing: f32,
) -> Result<MapNavBuild, String> {
    let bsp = Bsp::load(baseq2, map)?;
    let cm = Arc::new(CollisionModel::from_bsp(&bsp));
    let spawn_origins: Vec<[f32; 3]> = bsp.spawn_points().iter().map(|s| s.origin).collect();
    let fp = Fingerprint::from_bsp(&bsp, lift_penalty, spacing);

    // Try the disk cache if a directory was provided (per-spacing subdir).
    if let Some(dir) = cache_dir {
        let cache_path = dir
            .join(spacing_subdir(spacing))
            .join(format!("{map}.qnav"));
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
    }

    // Load-only: nav graphs are generated AHEAD of time with `generate-map-cache`, NEVER on
    // demand. A scenario runs `run_scenario` once per bot, so generating here meant N
    // concurrent regenerations of the same graph plus a cache-file write race. Instead, fail
    // with a clear instruction so the user generates the cache once, up front.
    let sp = spacing_subdir(spacing);
    Err(format!(
        "no fresh nav-graph cache for '{map}' at spacing {sp}. Generate it first:\n  \
         qbots generate-map-cache --map {map} --spacing {sp}"
    ))
}

/// Run the full build pipeline for `map` under `baseq2`: load the BSP, build the
/// collision model, sample the nav graph, seed DM spawns as nodes, and detect
/// ledge-drop jump edges. Returns `Err` only on load/parse failure or a BSP with
/// no models — never partial output.
pub fn generate_map_nav(
    baseq2: &Path,
    map: &str,
    lift_penalty: f32,
    spacing: f32,
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

    let mut graph = NavGraph::generate(&cm, bounds, spacing);
    let seeded = graph.seed_spawns(&cm, &spawn_origins);
    add_elevator_edges(&mut graph, &cm, &bsp, lift_penalty);
    add_train_edges(&mut graph, &cm, &bsp);
    let ladders = add_ladder_edges(&mut graph, &bsp);
    tracing::info!(map, ladders, "added ladder climb edges");
    graph.bridge_components(&cm, BRIDGE_HDIST);
    // `QBOTS_NO_PRUNE=1` skips the false-edge prune — a diagnostic for connectivity work
    // (Plan 35): if a map's spawn fragmentation persists with the prune off, the cause is in
    // generation/bridging, not the prune. (q2dm3: still 3-way split with it off.)
    let pruned = if std::env::var("QBOTS_NO_PRUNE").is_ok() {
        tracing::warn!(
            map,
            "QBOTS_NO_PRUNE set — skipping false-edge prune (diagnostic)"
        );
        0
    } else {
        graph.prune_long_blocked_edges(&cm, PRUNE_MAX_HD)
    };
    tracing::info!(map, pruned, "pruned long hull-blocked false edges");
    let added_jumps = graph.detect_jump_edges(&cm, JUMP_SPACING);
    // Fuse vertically-stacked floor components that connect only by a drop-off (q2dm3's
    // floors + the quad ledge) — a near-vertical jump-down detect_jump_edges' short probe
    // misses (Plan 42). Tight horizontal radius so only genuine vertical drops link.
    let jump_bridged =
        graph.bridge_components_via_jump(&cm, JUMP_BRIDGE_HDIST, JUMP_BRIDGE_MAX_FALL, 6);
    tracing::info!(
        map,
        jump_bridged,
        "bridged stacked floors via jump-down links"
    );
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
pub fn add_elevator_edges(
    graph: &mut NavGraph,
    cm: &CollisionModel,
    bsp: &Bsp,
    lift_penalty: f32,
) -> usize {
    let mut added = 0;
    // `func_plat`: auto-lowering platform, rests at top, travels down.
    for entity in bsp.find_class("func_plat") {
        if let Some(n) = try_add_plat(graph, cm, bsp, entity, lift_penalty) {
            added += n;
        }
    }
    // `func_door` used as a vertical lift (angle -1 = up, -2 = down). Geometrically
    // identical to a plat for nav: the brush's top surface occupies two z-levels.
    // Horizontal doors are skipped — their doorway floor lives in world model 0 and
    // is already sampled, so they never fragment the graph.
    for entity in bsp.find_class("func_door") {
        if let Some(n) = try_add_vertical_door(graph, cm, bsp, entity, lift_penalty) {
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

/// Resolve an entity's `"model" "*N"` field to the inline-model index `N` (Plan 42).
fn entity_model_index(entity: &BspEntity) -> Option<u32> {
    entity
        .fields
        .get("model")
        .and_then(|s| s.strip_prefix('*'))
        .and_then(|s| s.trim().parse().ok())
}

/// Horizontal radius (units) within which a moving-platform path endpoint connects to
/// existing walkable nodes (the board / dismount ledges). Plan 42. In the cache via VERSION.
pub const TRAIN_BOARD_RADIUS: f32 = 144.0;
/// Max vertical offset (units) between a candidate ride-surface height and the boarding-ledge
/// node ([`nearest_ground`], Plan 43/35) — you board at the platform-top height. In cache via VERSION.
pub const TRAIN_RIDE_DZ: f32 = 44.0;
/// Max height difference (units) between a train ride's board and dismount ledges — the platform
/// top is one level as it travels, so a pair straddling two heights isn't one ride. In cache via VERSION.
pub const TRAIN_SURFACE_DZ: f32 = 48.0;
/// Max horizontal offset (units) for a vertical jump-down floor bridge
/// ([`NavGraph::bridge_components_via_jump`], Plan 42). Tight — only near-vertical drops
/// between stacked floors link, never a horizontal false bridge. In the cache via VERSION.
pub const JUMP_BRIDGE_HDIST: f32 = 80.0;
/// Max fall height (units) for a vertical jump-down floor bridge (Plan 42). q2dm3's largest
/// stacked-floor drop (mid floor → lower) is ~144; the quad ledge drop is larger. 256 covers
/// them while staying within a survivable Q2 fall. In the cache via VERSION.
pub const JUMP_BRIDGE_MAX_FALL: f32 = 256.0;

/// Follow a `func_train`'s `path_corner` chain from its `target`, returning corner origins
/// in path order. Stops when the chain loops back on itself or a corner is missing; caps
/// iterations so a malformed map can't loop forever. (`g_func.c` train_next, `target` →
/// `targetname` links; the corner origin is the train's MIN-corner destination — see
/// `try_add_train`.)
fn train_corners(bsp: &Bsp, entity: &BspEntity) -> Vec<[f32; 3]> {
    use std::collections::{HashMap, HashSet};
    let mut by_name: HashMap<&str, &BspEntity> = HashMap::new();
    for e in &bsp.entities {
        if e.classname == "path_corner" {
            if let Some(tn) = e.fields.get("targetname") {
                by_name.insert(tn.as_str(), e);
            }
        }
    }
    let mut corners = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut cur = entity.fields.get("target").map(String::as_str);
    while let Some(name) = cur {
        if !seen.insert(name.to_string()) {
            break; // loop closed
        }
        let Some(c) = by_name.get(name) else { break };
        if let Some(o) = c.origin() {
            corners.push(o);
        }
        cur = c.fields.get("target").map(String::as_str);
        if corners.len() > 64 {
            break;
        }
    }
    corners
}

/// Add `EdgeKind::Ride` edges for every `func_train` moving platform (Plan 42).
///
/// A `func_train` carries a player along a `path_corner` loop. For nav we place a
/// platform-top "stand" node at each corner and link consecutive corners with ride edges
/// (the carry), then wire each stand node to nearby walkable ground (the board / dismount
/// ledges). The standable top is derived from Q2's positioning rule: `train_next` sets the
/// train so its **min corner** sits at the path_corner (`g_func.c:2310`, "targets origin
/// specifies the min point of the train"), so when the train rests at corner `C` the
/// brush spans `[C, C + size]` and its walkable top is `C.z + size.z`; the bot origin is
/// that `+ 24` (hull mins.z = −24), and the XY center is `C.xy + size.xy/2`.
///
/// Returns the number of edges added (ride + board/dismount walk edges).
pub fn add_train_edges(graph: &mut NavGraph, cm: &CollisionModel, bsp: &Bsp) -> usize {
    let mut added = 0;
    for entity in bsp.find_class("func_train") {
        added += try_add_train(graph, cm, bsp, entity).unwrap_or(0);
    }
    added
}

fn try_add_train(
    graph: &mut NavGraph,
    _cm: &CollisionModel,
    bsp: &Bsp,
    entity: &BspEntity,
) -> Option<usize> {
    use crate::navgraph::RideInfo;
    let model = entity_model(bsp, entity)?;
    let model_index = entity_model_index(entity)?;
    let size = [
        model.maxs[0] - model.mins[0],
        model.maxs[1] - model.mins[1],
        model.maxs[2] - model.mins[2],
    ];
    let corners = train_corners(bsp, entity);
    if corners.len() < 2 {
        return None;
    }

    // Per corner, the platform's wire entity origin when it rests there is `corner - mins`
    // (Q2 train_next, `g_func.c:2310`) — the brain matches the live entity against this.
    let ent_origin: Vec<[f32; 3]> = corners
        .iter()
        .map(|c| {
            [
                c[0] - model.mins[0],
                c[1] - model.mins[1],
                c[2] - model.mins[2],
            ]
        })
        .collect();

    // The standable top of a func_train relative to its path_corner differs by map: q2dm3's
    // loop trains (*3/*4) ride at the brush **top** (`corner.z + size.z` — the platform rises
    // out of the lava), but the central quad train (*10, origin-brushed) rides at the
    // **corner level** (`corner.z`, the brush is a stem hanging below). We don't parse origin
    // brushes, so we try BOTH heights and keep whichever finds reachable ground adjacent to the
    // path. The bot boards/dismounts from that SOLID GROUND (never the over-lava platform-top
    // coordinate, which is open air whenever the train isn't there).
    let mut rides = 0;
    for top_mode in [false, true] {
        // (corner index, ground node) for corners with adjacent ground at this ride height.
        let board: Vec<(usize, usize)> = corners
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let ride_z = if top_mode {
                    c[2] + size[2] + 24.0
                } else {
                    c[2] + 24.0
                };
                let probe = [c[0] + size[0] / 2.0, c[1] + size[1] / 2.0, ride_z];
                nearest_ground(graph, probe, TRAIN_BOARD_RADIUS, TRAIN_RIDE_DZ).map(|n| (i, n))
            })
            .collect();

        for a in 0..board.len() {
            for b in (a + 1)..board.len() {
                let (ci_a, na) = board[a];
                let (ci_b, nb) = board[b];
                if na == nb || graph.neighbors(na).iter().any(|&(x, _)| x == nb) {
                    continue; // same ledge, or already (walk/ride)-connected → skip
                }
                // Board & dismount must sit at a consistent height — the platform top is one
                // level as it travels, so a pair straddling two heights isn't one ride.
                if (graph.nodes[na][2] - graph.nodes[nb][2]).abs() > TRAIN_SURFACE_DZ {
                    continue;
                }
                let cost = dist3(graph.nodes[na], graph.nodes[nb]);
                graph.add_ride_edge(
                    na,
                    nb,
                    cost,
                    RideInfo {
                        board: graph.nodes[na],
                        far: graph.nodes[nb],
                        dismount: graph.nodes[nb],
                        model_index,
                        vertical: false,
                        board_ent: ent_origin[ci_a],
                        far_ent: ent_origin[ci_b],
                        ladder: false,
                    },
                );
                rides += 1;
            }
        }
    }

    tracing::info!(
        model = model_index,
        corners = corners.len(),
        ride_edges = rides,
        "func_train ride edges added"
    );
    Some(rides)
}

/// Nearest existing walkable node to `pos` within `max_h` horizontal and `max_dz` vertical
/// (Plan 43/35). Used to anchor a train's board/dismount and a ladder's base/top to solid
/// ground. Minimizes **3-D** distance (within the gates) so the chosen node sits at the right
/// HEIGHT, not just the nearest XY — a ladder top must snap to the top-floor ledge, never a
/// mid-height node that would make the bot "top out" early.
fn nearest_ground(graph: &NavGraph, pos: [f32; 3], max_h: f32, max_dz: f32) -> Option<usize> {
    let mut best = None;
    let mut best_d2 = f32::MAX;
    for (i, n) in graph.nodes.iter().enumerate() {
        let dz = n[2] - pos[2];
        if dz.abs() > max_dz {
            continue;
        }
        let dh2 = (n[0] - pos[0]).powi(2) + (n[1] - pos[1]).powi(2);
        if dh2 > max_h * max_h {
            continue;
        }
        let d2 = dh2 + dz * dz;
        if d2 < best_d2 {
            best_d2 = d2;
            best = Some(i);
        }
    }
    best
}

/// `CONTENTS_LADDER` (`files.h:368`) — a climbable brush volume.
const CONTENTS_LADDER: i32 = 0x2000_0000;
/// Horizontal radius (units) to find the floor node adjacent to a ladder's base/top. In cache via VERSION.
pub const LADDER_RADIUS: f32 = 96.0;
/// Vertical tolerance (units) when matching a ladder's base/top to a floor node. In cache via VERSION.
pub const LADDER_DZ: f32 = 56.0;

/// Axis-aligned bounding boxes of every `CONTENTS_LADDER` brush (Plan 35). A brush is an
/// intersection of half-spaces; the six axial planes give the AABB (`normal` ±1 on an axis →
/// `dist` is that face). Non-axial (bevel) planes are ignored.
fn ladder_aabbs(bsp: &Bsp) -> Vec<([f32; 3], [f32; 3])> {
    let mut out = Vec::new();
    for b in &bsp.brushes {
        if b.contents & CONTENTS_LADDER == 0 {
            continue;
        }
        let mut mn = [f32::MAX; 3];
        let mut mx = [f32::MIN; 3];
        for s in b.firstside..(b.firstside + b.numsides) {
            let Some(side) = bsp.brushsides.get(s as usize) else {
                continue;
            };
            let Some(pl) = bsp.planes.get(side.planenum as usize) else {
                continue;
            };
            for ax in 0..3 {
                if pl.normal[ax] > 0.99 {
                    mx[ax] = pl.dist;
                } else if pl.normal[ax] < -0.99 {
                    mn[ax] = -pl.dist;
                }
            }
        }
        if mn.iter().all(|v| v.is_finite()) && mx.iter().all(|v| v.is_finite()) {
            out.push((mn, mx));
        }
    }
    out
}

/// Add ladder climb edges (Plan 35). Each `CONTENTS_LADDER` brush connects a lower floor to an
/// upper one; our nav otherwise ignores ladders, leaving the upper level (q2dm3's comp0, the
/// quad/railgun overlook) cut off from the spawn floors. For each ladder, find the walkable
/// floor node adjacent to its base and to its top, and add a vertical **ladder** ride edge
/// (the brain hugs the ladder + presses `up` to climb). Returns the number of edges added.
pub fn add_ladder_edges(graph: &mut NavGraph, bsp: &Bsp) -> usize {
    let mut added = 0;
    for (mn, mx) in ladder_aabbs(bsp) {
        let cx = (mn[0] + mx[0]) / 2.0;
        let cy = (mn[1] + mx[1]) / 2.0;
        let bottom = nearest_ground(graph, [cx, cy, mn[2] + 24.0], LADDER_RADIUS, LADDER_DZ);
        let top = nearest_ground(graph, [cx, cy, mx[2] + 24.0], LADDER_RADIUS, LADDER_DZ);
        let (Some(bottom), Some(top)) = (bottom, top) else {
            continue;
        };
        if bottom == top || graph.neighbors(bottom).iter().any(|&(x, _)| x == top) {
            continue;
        }
        let center = [cx, cy, (mn[2] + mx[2]) / 2.0];
        let cost = (mx[2] - mn[2]).abs();
        graph.add_ride_edge(
            bottom,
            top,
            cost,
            crate::navgraph::RideInfo {
                board: graph.nodes[bottom],
                far: graph.nodes[top],
                dismount: graph.nodes[top],
                model_index: 0,
                vertical: true,
                board_ent: center,
                far_ent: center,
                ladder: true,
            },
        );
        added += 1;
        tracing::info!(
            center = ?[cx as i32, cy as i32, ((mn[2] + mx[2]) / 2.0) as i32],
            z = ?[mn[2] as i32, mx[2] as i32],
            "ladder climb edge added"
        );
    }
    added
}

/// Euclidean distance between two world points.
fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Add a two-level vertical lift between top-surface world z-values `z_hi` and `z_lo`
/// at the brush's XY center: a nav node per level, an edge for the ride itself, and
/// trace-checked edges from each node to nearby walkable floor nodes. Returns the
/// number of edges added (≥1 for the ride). `label` is for the debug log.
#[allow(clippy::too_many_arguments)]
fn add_lift(
    graph: &mut NavGraph,
    cm: &CollisionModel,
    model: &crate::bsp::Model,
    model_index: u32,
    z_hi: f32,
    z_lo: f32,
    label: &str,
    lift_penalty: f32,
) -> usize {
    let cx = (model.mins[0] + model.maxs[0]) / 2.0;
    let cy = (model.mins[1] + model.maxs[1]) / 2.0;
    // Nav node = player origin = floor_surface_z + 24 (hull_mins.z = -24).
    let top_node = [cx, cy, z_hi + 24.0];
    let bot_node = [cx, cy, z_lo + 24.0];

    tracing::debug!(cx, cy, z_hi, z_lo, travel = z_hi - z_lo, "{label}");

    let top_idx = graph.add_node(top_node);
    let bot_idx = graph.add_node(bot_node);
    // A vertical RIDE edge (Plan 43): the brain walks onto the pad and is carried, instead of
    // trying to "walk" the impossible vertical edge. `lift_penalty` still biases A* toward any
    // stair/ramp alternative (TODO(elevator-hack): remove with the multi-bot de-conflict in
    // Plan 31 — see context/elevator_todo.md), but a lift-only route (q2dm3 railgun) still uses it.
    graph.add_ride_edge(
        bot_idx,
        top_idx,
        (z_hi - z_lo).abs() + lift_penalty,
        crate::navgraph::RideInfo {
            board: bot_node,
            far: top_node,
            dismount: top_node,
            model_index,
            vertical: true,
            // Vertical lifts never use entity detection (the bot's presence summons the pad),
            // so these are unused; the pad's own position is its wire origin if ever needed.
            board_ent: bot_node,
            far_ent: top_node,
            ladder: false,
        },
    );
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
    lift_penalty: f32,
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
    let model_index = entity_model_index(entity).unwrap_or(0);
    Some(add_lift(
        graph,
        cm,
        model,
        model_index,
        model.maxs[2],
        model.maxs[2] - travel,
        "func_plat elevator bridge",
        lift_penalty,
    ))
}

fn try_add_vertical_door(
    graph: &mut NavGraph,
    cm: &CollisionModel,
    bsp: &Bsp,
    entity: &BspEntity,
    lift_penalty: f32,
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
    let model_index = entity_model_index(entity).unwrap_or(0);
    Some(add_lift(
        graph,
        cm,
        model,
        model_index,
        z_hi,
        z_lo,
        "func_door lift bridge",
        lift_penalty,
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
