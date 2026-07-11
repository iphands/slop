//! Nav graph — auto-generated waypoints + A* pathfinding.
//!
//! The genuinely original part of the world model (no bot archive did this externally):
//! sample walkable floor positions on a grid, connect neighbors whose edge clears a
//! player-box trace, then A* over the graph.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};

use rayon::prelude::*;

use crate::collision::{
    CollisionModel, CONTENTS_LAVA, CONTENTS_SLIME, CONTENTS_WATER, MASK_SOLID, MASK_WATER,
};

/// Q2 standing player hull (`VEC_HULL_MIN/MAX`): the bbox traces use.
pub const HULL_MINS: [f32; 3] = [-16.0, -16.0, -24.0];
pub const HULL_MAXS: [f32; 3] = [16.0, 16.0, 32.0];
/// Max walkable height delta between adjacent waypoints — the step-**climb** height.
/// Matches Q2's real `STEPSIZE`: `pmove.c:32` `#define STEPSIZE 18`. Do not confuse
/// with arrival-tolerance constants (e.g. `WP_REACH_DZ` in `brain::nav`) or trace-lift
/// offsets (e.g. `STEPSIZE` in `brain::recover`) that happen to use a numerically nearby
/// value for unrelated reasons.
pub const STEP: f32 = 18.0;
/// Maximum height difference for which the stair-climb trace is attempted instead of
/// immediate rejection. Must be large enough for the tallest single staircase flight
/// in any q2dm* map. Empirically: q2dm3 has staircase nodes 144u apart vertically at
/// the same XY column; 160u covers that with room. The stair trace itself rejects
/// geometry-blocked paths, so raising this value is safe — it only causes more traces
/// to be attempted, never creates false edges through walls or ceilings.
/// Must stay in the cache fingerprint (`mapcache::Fingerprint`) so stale caches
/// auto-invalidate on change.
pub const STAIR_MAX: f32 = 160.0;
/// Vertical spacing (units) between submerged swim nodes sampled in a water column
/// (Plan 39). Coarse enough to keep the node count bounded, fine enough that adjacent
/// submerged nodes are within a single swim-edge vertical reach. In the cache
/// fingerprint so changing it auto-invalidates stale caches.
pub const SWIM_SPACING: f32 = 32.0;
/// Edge-cost multiplier for **swim↔swim** edges (Plan 39). Q2 water movement is ~0.5×
/// ground speed (`pmove.c:579 wishspeed *= 0.5`), so a swim edge costs ~2× an equal-length
/// walk edge — this keeps A* preferring a dry route whenever one exists. Entry/exit edges
/// (one dry endpoint) are NOT scaled (walking/falling in is cheap). In the cache fingerprint.
pub const SWIM_COST_FACTOR: f32 = 2.0;
/// Max |dz| (units) for any water edge (swim↔swim vertical link, or dry↔water entry/exit).
/// `SWIM_SPACING * 1.5` keeps vertically-adjacent submerged nodes linkable while bounding
/// a single edge's vertical reach (no full-pool jumps). Entry/exit dz is always smaller.
pub const WATER_VLINK: f32 = SWIM_SPACING * 1.5;
/// Reduced player hull for swim traces (Plan 39). A submerged bot is not floor-constrained
/// and can occupy tighter space than the standing hull; the full `HULL_*` is too strict for
/// narrow tunnels. Modest so real geometry (`MASK_SOLID`) still blocks false edges.
pub const SWIM_HULL_MINS: [f32; 3] = [-12.0, -12.0, -12.0];
pub const SWIM_HULL_MAXS: [f32; 3] = [12.0, 12.0, 12.0];
/// Target WORLD-UNIT radius that `generate()` connects each node within. This is the
/// load-bearing quantity — NOT the cell count. Experiments (q2dm1, 2026-06-17) showed
/// the graph quality depends on the absolute connection radius, not the grid: holding
/// this at ~72u, spawn-to-spawn stays good across grid spacings; letting it drift (by
/// keeping a fixed cell count while changing the grid) re-fragments the graph (e.g. 24→16
/// with CONNECT_CELLS fixed dropped the radius 72→48u → 16/24). So the cell radius is
/// DERIVED from this and the grid via [`connect_cells`], which means GRID_SPACING can be
/// changed freely and the connection radius stays correct automatically.
/// In the cache fingerprint so changing it invalidates stale caches.
pub const CONNECT_RADIUS: f32 = 72.0;

/// Hard cap on the per-axis cell connection radius. Without it, a FIXED world-unit
/// `CONNECT_RADIUS` at a fine grid reaches many cells (72u at 12u spacing = ±6 cells →
/// ~90 edges/node), turning the graph into a dense MESH that A* routes along walls and
/// bots jam in. The missed-links that motivated a wide radius are a COARSE-grid artifact
/// (no intermediate node sampled); at a fine grid the dense sampling already provides the
/// intermediate nodes, so ±3 cells suffices for connectivity and keeps the graph SPARSE.
const MAX_CONNECT_CELLS: i32 = 3;

/// Per-axis grid-cell connection radius for `spacing`: cover [`CONNECT_RADIUS`] (ceil) but
/// never exceed [`MAX_CONNECT_CELLS`] so fine grids stay sparse. At spacing 24 → 3 cells
/// (72u, the original); at spacing 12 → 3 cells (36u, not the dense ±6). `≥1` always.
pub fn connect_cells(spacing: f32) -> i32 {
    ((CONNECT_RADIUS / spacing).ceil() as i32).clamp(1, MAX_CONNECT_CELLS)
}
/// Minimum edge cost in the weighted pathfinder, so a popularity overlay can't
/// drive an edge to zero/negative (Plan 08 T3).
const EPS: f32 = 1.0;

/// The kind of an edge in the nav graph (Plan 14 T2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeKind {
    /// Normal walk/ground-movement edge.
    Walk,
    /// Ledge-drop jump edge: the bot should press jump + forward when leaving
    /// the source node. `launch_yaw` is the world-space yaw (degrees) to face.
    Jump { launch_yaw: f32 },
    /// Swim edge (Plan 39): at least one endpoint is a water node. The bot moves
    /// through the water volume in 3-D (vertical thrust via `intent.up`/pitch) rather
    /// than walking. Includes dry→water entry and water→dry exit (the railgun climb-out).
    Swim,
    /// Ride edge (Plan 42): the bot crosses this edge by riding a moving platform
    /// (`func_train`). It walks to the board point, waits for the platform to arrive
    /// (read live from frames), is carried to the far end, then steps off. The
    /// per-edge [`RideInfo`] (board / far / dismount positions) is fetched via
    /// [`NavGraph::ride_info`].
    Ride,
}

/// Per-edge data for an [`EdgeKind::Ride`] moving-platform edge (Plan 42). The brain reads
/// this to drive the approach → wait → board → ride → dismount sequence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RideInfo {
    /// Where the bot waits and boards — the platform's path endpoint nearest the source node.
    pub board: [f32; 3],
    /// The platform's far path endpoint — the bot dismounts when the platform nears it.
    pub far: [f32; 3],
    /// The walkable ground node the bot steps onto when dismounting.
    pub dismount: [f32; 3],
    /// BSP inline-model index of the `func_train`/`func_plat` (`*N`), for matching the live entity.
    pub model_index: u32,
    /// `true` for a **vertical lift** (`func_plat`/`func_door`): the bot walks onto the pad and
    /// is carried straight up/down (no horizontal wait-for-arrival). `false` for a horizontal
    /// `func_train` the bot boards at a path endpoint when it arrives.
    pub vertical: bool,
    /// Expected **wire entity origin** of the platform when it sits at the board corner
    /// (`path_corner - model.mins`, Q2's `train_next` rule). The brush model's wire origin is
    /// this offset, NOT the stand-center, so the brain matches the live entity against this to
    /// know the train has arrived (Plan 43). Unused for vertical lifts.
    pub board_ent: [f32; 3],
    /// Expected wire entity origin at the far corner (the reverse ride's board). See `board_ent`.
    pub far_ent: [f32; 3],
    /// `true` for a **ladder** climb (`CONTENTS_LADDER`, Plan 35): the bot hugs the ladder and
    /// presses `up` to climb (Q2 `PM_AddCurrents` ladder rule). `board_ent`/`far_ent` hold the
    /// ladder center (the facing target). `false` for func_train/func_plat movers.
    pub ladder: bool,
    /// Constant offset from a train's live **wire origin** to its **standable top-center**
    /// (Plan 43): `platform_top = entity.origin + stand_offset`. The brain uses this to track
    /// the moving platform's top and stay centered on it (over lava) instead of sliding off.
    /// Zero for lifts/ladders.
    pub stand_offset: [f32; 3],
}

/// A navigation graph: waypoints (bot-origin positions) + LOS-checked edges.
pub struct NavGraph {
    pub nodes: Vec<[f32; 3]>,
    adj: Vec<Vec<(usize, f32)>>,         // (neighbor index, edge cost)
    jump_edges: HashSet<(usize, usize)>, // (from, to) pairs that are jump edges
    jump_yaws: HashMap<(usize, usize), f32>, // launch_yaw per jump edge
    /// Directed `(from, to)` pairs that are swim edges (Plan 39). Bidirectional swim
    /// edges store both directions, so `edge_kind`/`is_swim_edge` need only one lookup.
    swim_edges: HashSet<(usize, usize)>,
    /// Indices of nodes that lie inside a water volume (Plan 39). Used by the brain to
    /// drive vertical swim movement and to protect swim edges from the false-edge prune.
    water_nodes: HashSet<usize>,
    /// Directed `(from, to)` pairs that are ride edges (Plan 42). Both directions of a
    /// bidirectional ride are stored so `edge_kind`/`is_ride_edge` need one lookup.
    ride_edges: HashSet<(usize, usize)>,
    /// Per-ride-edge metadata (board/far/dismount positions + model index), keyed by the
    /// directed `(from, to)` pair (Plan 42).
    ride_info: HashMap<(usize, usize), RideInfo>,
}

impl NavGraph {
    /// Sample walkable floor on a `spacing`-unit grid over `bounds`, connect 8-neighbors
    /// whose edge is clear and step is small. Grid sampling and edge connectivity are
    /// both parallelized with rayon; node ordering is sorted by grid key so two runs
    /// on the same BSP produce byte-identical caches.
    pub fn generate(cm: &CollisionModel, bounds: ([f32; 3], [f32; 3]), spacing: f32) -> Self {
        // --- Phase 1: collect all candidate (x, y) grid columns ------------------
        let mut columns: Vec<(f32, f32, i32, i32)> = Vec::new();
        let mut x = bounds.0[0];
        while x <= bounds.1[0] {
            let mut y = bounds.0[1];
            while y <= bounds.1[1] {
                let gx = (x / spacing).round() as i32;
                let gy = (y / spacing).round() as i32;
                columns.push((x, y, gx, gy));
                y += spacing;
            }
            x += spacing;
        }

        // --- Phase 2: parallel multi-floor probe (CollisionModel is Sync) ----------
        // Each column can yield multiple floors (e.g. roof-top + indoor level below).
        // flat_map_iter keeps rayon work-stealing while returning a sequential iterator
        // per column; the move captures (gx,gy) into each emitted pair.
        // Each emitted node carries an `is_water` tag (Plan 39): dry floor nodes from
        // `floor_waypoints_multi` (false) plus submerged/surface nodes from
        // `water_waypoints_multi` (true). Water nodes are connected in 3-D by the swim
        // edge pass below; dry nodes use the existing XY-grid stair logic.
        let mut hits: Vec<((i32, i32), [f32; 3], bool)> = columns
            .par_iter()
            .flat_map_iter(|&(x, y, gx, gy)| {
                let dry = floor_waypoints_multi(cm, x, y, bounds)
                    .into_iter()
                    .map(move |wp| ((gx, gy), wp, false));
                let wet = water_waypoints_multi(cm, x, y, bounds)
                    .into_iter()
                    .map(move |wp| ((gx, gy), wp, true));
                dry.chain(wet)
            })
            .collect();

        // Sort by (grid_key, z, is_water) so node indices are deterministic across runs.
        hits.sort_by(|((ax, ay), aw, awet), ((bx, by), bw, bwet)| {
            (*ax, *ay)
                .cmp(&(*bx, *by))
                .then_with(|| aw[2].total_cmp(&bw[2]))
                .then_with(|| awet.cmp(bwet))
        });

        // Grid maps each column to all node indices it contains (ordered by z asc).
        let mut nodes: Vec<[f32; 3]> = Vec::with_capacity(hits.len());
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::with_capacity(hits.len());
        let mut water_nodes: HashSet<usize> = HashSet::new();
        for ((gx, gy), wp, is_water) in hits {
            let idx = nodes.len();
            grid.entry((gx, gy)).or_default().push(idx);
            if is_water {
                water_nodes.insert(idx);
            }
            nodes.push(wp);
        }

        // --- Phase 3: parallel edge computation -----------------------------------
        // For each node, look at the 8 adjacent columns and try to connect to every
        // node in each column whose |dz| is within STAIR_MAX. This handles the case
        // where adjacent columns have nodes on different floors: the z-distance filter
        // keeps only floor-level peers; the stair trace rejects walls.
        // No shared mutable state during the parallel section.
        // Cell radius derived from the world-unit CONNECT_RADIUS, so changing `spacing`
        // keeps the absolute connection radius constant (see CONNECT_RADIUS docs).
        let cells = connect_cells(spacing);
        // Each edge carries an `is_swim` flag so the sequential merge below can populate
        // `swim_edges` (Plan 39). Water-involved pairs use 3-D swim connectivity; dry pairs
        // keep the XY-grid stair logic.
        let per_node: Vec<Vec<(usize, f32, bool)>> = nodes
            .par_iter()
            .enumerate()
            .map(|(i, &a)| {
                let gx = (a[0] / spacing).round() as i32;
                let gy = (a[1] / spacing).round() as i32;
                let a_water = water_nodes.contains(&i);
                let mut edges: Vec<(usize, f32, bool)> = Vec::new();
                // Connect a neighbourhood of ±cells grid cells, not just the 8 immediate
                // neighbours. A ±1 (24u) connection misses real walkable links that span
                // 2-4 cells — e.g. across a ramp/step where the intermediate column has no
                // sampled node — which fragments the graph into dozens of false components
                // (proven by tools/compgaps: 934 missed walkable links on q2dm1). The
                // per-pair hull/stair check below still rejects wall-separated pairs, so
                // widening only adds genuinely walkable edges.
                for ddx in -cells..=cells {
                    for ddy in -cells..=cells {
                        // Same column (0,0): only water nodes link here (vertical swim
                        // lattice). Dry floors never stack walkably in one column.
                        if ddx == 0 && ddy == 0 {
                            if !a_water {
                                continue;
                            }
                            if let Some(col) = grid.get(&(gx, gy)) {
                                for &j in col {
                                    if j == i || !water_nodes.contains(&j) {
                                        continue;
                                    }
                                    if let Some(c) = try_swim_edge(cm, &a, &nodes[j], true) {
                                        edges.push((j, c, true));
                                    }
                                }
                            }
                            continue;
                        }
                        let Some(col) = grid.get(&(gx + ddx, gy + ddy)) else {
                            continue;
                        };
                        for &j in col {
                            if j == i {
                                continue;
                            }
                            let b = nodes[j];
                            // Exact per-axis connection window: the cell scan over-covers
                            // (ceil), so trim to ±CONNECT_RADIUS world units. Keeps the
                            // absolute radius identical for every grid spacing (grid=24 →
                            // ±72u = the old ±3 cells; grid=16 → ±72u, not the rounded 80u).
                            if (b[0] - a[0]).abs() > CONNECT_RADIUS
                                || (b[1] - a[1]).abs() > CONNECT_RADIUS
                            {
                                continue;
                            }
                            let b_water = water_nodes.contains(&j);
                            // Water-involved pair (swim↔swim, or dry↔water entry/exit):
                            // 3-D reduced-hull connectivity that bypasses the STEP/STAIR
                            // gates. The exit edge (water-surface → dry railgun ledge) is
                            // the critical bridge that fuses the railgun room (Plan 39 T4).
                            if a_water || b_water {
                                if let Some(c) = try_swim_edge(cm, &a, &b, a_water && b_water) {
                                    edges.push((j, c, true));
                                }
                                continue;
                            }
                            let dz = b[2] - a[2];
                            if dz.abs() > STAIR_MAX {
                                continue; // too steep for stairs — cliff or void
                            }
                            let ok = if dz.abs() <= STEP {
                                // Flat or gentle slope: try direct hull trace first.
                                // Fall back to the step-climb trace when the direct trace
                                // fails — stair risers can clip the diagonal even for
                                // small height deltas.
                                // The hull trace flies at body height and clears freely
                                // over a lava trench narrower than CONNECT_RADIUS between
                                // two safe rim nodes — a passing trace must ALSO show
                                // continuous non-deadly floor (Plan 50 E1, cache v22).
                                let t = cm.trace(&a, &b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                                if !t.startsolid && t.fraction >= 1.0 {
                                    segment_has_floor(cm, a, b)
                                } else if dz.abs() > 0.5 {
                                    let (lower, upper) = if dz > 0.0 { (a, b) } else { (b, a) };
                                    walkable_stair(cm, lower, upper)
                                } else {
                                    false
                                }
                            } else {
                                // Height diff in (STEP, STAIR_MAX]: the diagonal trace
                                // would clip stair risers. Use the step-climb trace that
                                // mirrors Q2 pmove's up→forward movement pattern.
                                let (lower, upper) = if dz > 0.0 { (a, b) } else { (b, a) };
                                walkable_stair(cm, lower, upper)
                            };
                            if ok {
                                edges.push((j, dist(&a, &b), false));
                            }
                        }
                    }
                }
                edges
            })
            .collect();

        // Merge per-node edge lists sequentially: build the adjacency and record swim
        // edges. Each direction is computed independently, so a bidirectional swim edge
        // appears in both endpoints' lists → both `(i,j)` and `(j,i)` enter `swim_edges`.
        let mut adj: Vec<Vec<(usize, f32)>> = vec![Vec::new(); nodes.len()];
        let mut swim_edges: HashSet<(usize, usize)> = HashSet::new();
        for (i, list) in per_node.into_iter().enumerate() {
            for (j, cost, is_swim) in list {
                adj[i].push((j, cost));
                if is_swim {
                    swim_edges.insert((i, j));
                }
            }
        }

        NavGraph {
            nodes,
            adj,
            jump_edges: HashSet::new(),
            jump_yaws: HashMap::new(),
            swim_edges,
            water_nodes,
            ride_edges: HashSet::new(),
            ride_info: HashMap::new(),
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    /// World position of node `idx`. Used to express a roam waypoint as a concrete
    /// position goal for backends (navmesh) that don't index this graph's nodes.
    pub fn node_pos(&self, idx: usize) -> [f32; 3] {
        self.nodes[idx]
    }
    pub fn edge_count(&self) -> usize {
        self.adj.iter().map(|e| e.len()).sum()
    }

    /// Classify every undirected edge for the prune, in PARALLEL (the trace work). Order
    /// is preserved (`flat_map_iter` over ascending node indices, then adjacency order), so
    /// the output equals the sequential classifier — see `classify_prune_edges_seq` and the
    /// `prune_classify_par_matches_seq` test.
    fn classify_prune_edges_par(&self, cm: &CollisionModel, max_hd: f32) -> Vec<EdgeClass> {
        let nodes = &self.nodes;
        let adj = &self.adj;
        let swim = &self.swim_edges;
        let ride = &self.ride_edges;
        (0..nodes.len())
            .into_par_iter()
            .flat_map_iter(|a| {
                adj[a].iter().filter_map(move |&(b, _)| {
                    // Swim edges (Plan 39) are validated by a 3-D reduced-hull trace, and ride
                    // edges (Plan 42) cross open space on a moving platform — neither follows a
                    // walk/stair line, so the prune's hull check would falsely flag them. Keep
                    // them as trustworthy (visited once per undirected pair, a < b).
                    if a < b && (swim.contains(&(a, b)) || ride.contains(&(a, b))) {
                        return Some(EdgeClass::Trustworthy(a, b));
                    }
                    classify_prune_edge(nodes, cm, max_hd, a, b)
                })
            })
            .collect()
    }

    /// Sequential twin of [`Self::classify_prune_edges_par`] — same logic, single-threaded.
    /// Kept for the equality test that guards the parallel version.
    #[cfg(test)]
    fn classify_prune_edges_seq(&self, cm: &CollisionModel, max_hd: f32) -> Vec<EdgeClass> {
        let mut v = Vec::new();
        for a in 0..self.nodes.len() {
            for &(b, _) in &self.adj[a] {
                if a < b && (self.swim_edges.contains(&(a, b)) || self.ride_edges.contains(&(a, b)))
                {
                    v.push(EdgeClass::Trustworthy(a, b));
                    continue;
                }
                if let Some(c) = classify_prune_edge(&self.nodes, cm, max_hd, a, b) {
                    v.push(c);
                }
            }
        }
        v
    }

    /// Prune **redundant** hull-blocked edges that the bot cannot physically follow,
    /// while keeping every load-bearing one. Two classes of false edge are removed:
    ///
    /// - **Long** blocked edges (`hd > max_hd`): bridge/seed passes add these (up to
    ///   `BRIDGE_HDIST`); `walkable_stair` accepts them by sampling surfaces across open
    ///   space, but straight-line steering clips the wall between → unfollowable.
    /// - **Flat** blocked edges (`|dz| ≤ STEP`): a same-level edge with a wall in the
    ///   straight line is unambiguously false — there is no stair to climb, so the bot
    ///   just jams into the wall. These cause replan churn (the bot is sent to a waypoint
    ///   65-120u away across a wall, gives up, replans to another false-flat node, repeats)
    ///   and tank path efficiency. They survived the long-only prune (hd 96-120 < 144).
    ///
    /// Real **short steep** edges (`|dz| > STEP`, `hd ≤ max_hd`) are kept untraced — a
    /// blocked straight line is NORMAL for a staircase the bot climbs via pmove stepping.
    ///
    /// Connectivity-preserving: union all trustworthy edges, then keep a candidate only if
    /// its endpoints are still in different components (a real bridge), else drop it.
    /// Removes both directions. Returns directed edges removed. Re-check spawn connectivity.
    pub fn prune_long_blocked_edges(&mut self, cm: &CollisionModel, max_hd: f32) -> usize {
        let n = self.nodes.len();

        // Phase 1 (PARALLEL): classify every undirected edge. ALL the cm.trace /
        // walkable_stair calls happen here — pure read-only collision queries with no
        // shared mutable state — so they fan out across cores via rayon. This is the
        // dominant cost (millions of traces at fine grids). `flat_map_iter` preserves
        // order, so the classification Vec is identical to a sequential pass (asserted by
        // the `prune_classify_par_matches_seq` test), which keeps the result deterministic.
        let classes = self.classify_prune_edges_par(cm, max_hd);

        // Phase 2 (SEQUENTIAL): union-find merge. Final component membership is independent
        // of union order, but the keep/drop decision must process candidates in sorted order
        // against the accumulated component state, so this stays single-threaded.
        let mut uf = UnionFind::new(n);
        let mut candidates: Vec<(usize, usize, f32)> = Vec::new();
        for c in classes {
            match c {
                EdgeClass::Trustworthy(a, b) => uf.union(a, b),
                EdgeClass::Candidate(a, b, hd2) => candidates.push((a, b, hd2)),
            }
        }
        // Shortest candidate bridges first so we keep the tightest real connection.
        candidates.sort_by(|x, y| x.2.partial_cmp(&y.2).unwrap());
        let mut drop: Vec<(usize, usize)> = Vec::new();
        for (a, b, _) in candidates {
            if uf.find(a) == uf.find(b) {
                drop.push((a, b)); // already connected without it → redundant false edge
            } else {
                uf.union(a, b); // load-bearing bridge → keep
            }
        }
        let mut removed = 0;
        for (a, b) in drop {
            let before = self.adj[a].len();
            self.adj[a].retain(|&(nb, _)| nb != b);
            removed += before - self.adj[a].len();
            let before = self.adj[b].len();
            self.adj[b].retain(|&(nb, _)| nb != a);
            removed += before - self.adj[b].len();
            self.jump_edges.remove(&(a, b));
            self.jump_edges.remove(&(b, a));
            self.jump_yaws.remove(&(a, b));
            self.jump_yaws.remove(&(b, a));
            self.swim_edges.remove(&(a, b));
            self.swim_edges.remove(&(b, a));
            self.ride_edges.remove(&(a, b));
            self.ride_edges.remove(&(b, a));
            self.ride_info.remove(&(a, b));
            self.ride_info.remove(&(b, a));
        }
        removed
    }

    /// Build a graph directly from nodes + adjacency (each adjacency entry is
    /// `(neighbor index, edge cost)`). Intended for tests that need a nav graph
    /// without running the BSP sampler. `nodes` and `adj` must align in length.
    pub fn from_raw(nodes: Vec<[f32; 3]>, adj: Vec<Vec<(usize, f32)>>) -> Self {
        assert_eq!(
            nodes.len(),
            adj.len(),
            "nodes and adjacency vectors must have equal length"
        );
        Self {
            nodes,
            adj,
            jump_edges: HashSet::new(),
            jump_yaws: HashMap::new(),
            swim_edges: HashSet::new(),
            water_nodes: HashSet::new(),
            ride_edges: HashSet::new(),
            ride_info: HashMap::new(),
        }
    }

    /// Build a graph from pre-serialized components (mapcache deserialization).
    /// `jump_triples` is a list of `(from, to, launch_yaw)`.
    pub fn from_raw_with_jumps(
        nodes: Vec<[f32; 3]>,
        adj: Vec<Vec<(usize, f32)>>,
        jump_triples: Vec<(usize, usize, f32)>,
    ) -> Self {
        assert_eq!(
            nodes.len(),
            adj.len(),
            "nodes and adjacency vectors must have equal length"
        );
        let mut jump_edges = HashSet::new();
        let mut jump_yaws = HashMap::new();
        for (from, to, yaw) in jump_triples {
            jump_edges.insert((from, to));
            jump_yaws.insert((from, to), yaw);
        }
        Self {
            nodes,
            adj,
            jump_edges,
            jump_yaws,
            swim_edges: HashSet::new(),
            water_nodes: HashSet::new(),
            ride_edges: HashSet::new(),
            ride_info: HashMap::new(),
        }
    }

    /// Inject pre-serialized swim edges + water-node tags (mapcache deserialization,
    /// Plan 39). Both directions of each bidirectional swim edge are stored.
    pub fn set_swim_and_water(&mut self, swim: Vec<(usize, usize)>, water: Vec<usize>) {
        self.swim_edges = swim.into_iter().collect();
        self.water_nodes = water.into_iter().collect();
    }

    /// Swim edges and water-node indices for serialization (Plan 39), each sorted for
    /// determinism. Swim edges are returned directed exactly as stored (both directions).
    pub fn raw_swim_and_water(&self) -> (Vec<(usize, usize)>, Vec<usize>) {
        let mut swim: Vec<(usize, usize)> = self.swim_edges.iter().copied().collect();
        swim.sort_unstable();
        let mut water: Vec<usize> = self.water_nodes.iter().copied().collect();
        water.sort_unstable();
        (swim, water)
    }

    /// True if node `idx` lies inside a water volume (Plan 39).
    pub fn is_water_node(&self, idx: usize) -> bool {
        self.water_nodes.contains(&idx)
    }

    /// True if the directed edge `(a, b)` is a swim edge (Plan 39).
    pub fn is_swim_edge(&self, a: usize, b: usize) -> bool {
        self.swim_edges.contains(&(a, b))
    }

    /// Add a bidirectional ride edge (Plan 42) between nodes `a` (board side) and `b`
    /// (dismount side), with cost `cost`. `info_ab` describes the ride taken when crossing
    /// `a → b`; the reverse `b → a` gets the board/dismount swapped (same far endpoints).
    pub fn add_ride_edge(&mut self, a: usize, b: usize, cost: f32, info_ab: RideInfo) {
        if a >= self.adj.len() || b >= self.adj.len() {
            return;
        }
        self.adj[a].push((b, cost));
        self.adj[b].push((a, cost));
        self.ride_edges.insert((a, b));
        self.ride_edges.insert((b, a));
        self.ride_info.insert((a, b), info_ab);
        // Reverse ride: board where the a→b ride dismounted, ride back to the a side.
        self.ride_info.insert(
            (b, a),
            RideInfo {
                board: info_ab.far,
                far: info_ab.board,
                dismount: self.nodes[a],
                model_index: info_ab.model_index,
                vertical: info_ab.vertical,
                board_ent: info_ab.far_ent,
                far_ent: info_ab.board_ent,
                ladder: info_ab.ladder,
                stand_offset: info_ab.stand_offset,
            },
        );
    }

    /// True if the directed edge `(a, b)` is a ride edge (Plan 42).
    pub fn is_ride_edge(&self, a: usize, b: usize) -> bool {
        self.ride_edges.contains(&(a, b))
    }

    /// The [`RideInfo`] for the directed ride edge `(from, to)`, if it is one (Plan 42).
    pub fn ride_info(&self, from: usize, to: usize) -> Option<RideInfo> {
        self.ride_info.get(&(from, to)).copied()
    }

    /// Inject pre-serialized ride edges (mapcache deserialization, Plan 42). Each tuple is a
    /// directed `(from, to, RideInfo)`; both directions are stored explicitly by the caller.
    pub fn set_rides(&mut self, rides: Vec<(usize, usize, RideInfo)>) {
        for (from, to, info) in rides {
            self.ride_edges.insert((from, to));
            self.ride_info.insert((from, to), info);
        }
    }

    /// Ride edges for serialization (Plan 42), sorted for determinism. Returns directed
    /// `(from, to, RideInfo)` exactly as stored (both directions).
    pub fn raw_rides(&self) -> Vec<(usize, usize, RideInfo)> {
        let mut rides: Vec<(usize, usize, RideInfo)> = self
            .ride_info
            .iter()
            .map(|(&(f, t), &info)| (f, t, info))
            .collect();
        rides.sort_by_key(|&(f, t, _)| (f, t));
        rides
    }

    /// Borrow the graph's raw components for serialization. Returns clones of the
    /// internal data so the graph remains usable afterward. `jump_triples` is sorted
    /// `(from, to, launch_yaw)` for determinism across runs.
    #[allow(clippy::type_complexity)]
    pub fn raw_parts(
        &self,
    ) -> (
        Vec<[f32; 3]>,
        Vec<Vec<(usize, f32)>>,
        Vec<(usize, usize, f32)>,
    ) {
        let mut jump_triples: Vec<(usize, usize, f32)> = self
            .jump_yaws
            .iter()
            .map(|(&(f, t), &y)| (f, t, y))
            .collect();
        jump_triples.sort_by_key(|&(f, t, _)| (f, t));
        (self.nodes.clone(), self.adj.clone(), jump_triples)
    }

    /// Add a node to the graph and return its index.
    pub fn add_node(&mut self, node: [f32; 3]) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(node);
        self.adj.push(Vec::new());
        idx
    }

    /// Add a bidirectional edge between two nodes.
    pub fn add_edge(&mut self, a: usize, b: usize, cost: f32) {
        if a < self.adj.len() && b < self.adj.len() {
            self.adj[a].push((b, cost));
            self.adj[b].push((a, cost));
        }
    }

    /// Connected components (BFS). Useful for diagnosing multi-level fragmentation.
    /// WARNING: O(n + e) - expensive for large graphs. Cache the result if calling multiple times.
    pub fn components(&self) -> Vec<Vec<usize>> {
        let n = self.nodes.len();
        // Group over the UNDIRECTED view of the adjacency (Plan 35 T3). Walk edges are stored
        // bidirectionally, but jump-down bridges are ONE-WAY (`adj[hi] → lo` only) — a
        // forward-only DFS made the grouping visit-order-dependent: if the lower floor was
        // visited first, a hi→lo drop edge never merged the pair even though A* happily paths
        // across it (q2dm7's play areas were split this way while a route existed). One-way
        // drops COUNT as connectivity by design (the q2dm3 jump-bridge precedent); build the
        // reverse adjacency so the DFS sees them from both sides.
        let mut rev: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (u, nbs) in self.adj.iter().enumerate() {
            for &(nb, _) in nbs {
                rev[nb].push(u);
            }
        }
        let mut seen = vec![false; n];
        let mut comps = Vec::new();
        for start in 0..n {
            if seen[start] {
                continue;
            }
            let mut comp = Vec::new();
            let mut stack = vec![start];
            seen[start] = true;
            while let Some(u) = stack.pop() {
                comp.push(u);
                for &(nb, _) in &self.adj[u] {
                    if !seen[nb] {
                        seen[nb] = true;
                        stack.push(nb);
                    }
                }
                for &nb in &rev[u] {
                    if !seen[nb] {
                        seen[nb] = true;
                        stack.push(nb);
                    }
                }
            }
            comps.push(comp);
        }
        comps.sort_by_key(|b| std::cmp::Reverse(b.len()));
        comps
    }

    /// Number of outgoing edges from node `idx` (for diagnostics).
    pub fn adj_count(&self, idx: usize) -> usize {
        self.adj.get(idx).map_or(0, |v| v.len())
    }

    /// Outgoing edges `(neighbor_idx, cost)` from node `idx` (for diagnostics/tools).
    pub fn neighbors(&self, idx: usize) -> &[(usize, f32)] {
        self.adj.get(idx).map_or(&[], |v| v.as_slice())
    }

    /// Sorted list of unique Z-levels of all neighbors of node `idx` (for diagnostics).
    pub fn adj_neighbor_z_levels(&self, idx: usize) -> Vec<i32> {
        let Some(neighbors) = self.adj.get(idx) else {
            return vec![];
        };
        let mut zs: std::collections::BTreeSet<i32> = std::collections::BTreeSet::new();
        for &(nb, _) in neighbors {
            if let Some(p) = self.nodes.get(nb) {
                zs.insert(p[2] as i32);
            }
        }
        zs.into_iter().collect()
    }

    /// Nearest waypoint to `p` (linear; graphs are modest).
    pub fn nearest(&self, p: &[f32; 3]) -> Option<usize> {
        self.nodes
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| dist2(a, p).total_cmp(&dist2(b, p)))
            .map(|(i, _)| i)
    }

    /// BFS from `start`; among all reachable nodes, return the one nearest to `target`.
    /// Used when start and a desired goal are in different components.
    pub fn nearest_reachable_from(&self, start: usize, target: &[f32; 3]) -> Option<usize> {
        let n = self.nodes.len();
        let mut seen = vec![false; n];
        let mut queue = std::collections::VecDeque::new();
        seen[start] = true;
        queue.push_back(start);
        let mut best: Option<(usize, f32)> = None;
        while let Some(u) = queue.pop_front() {
            let d = dist2(&self.nodes[u], target);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((u, d));
            }
            for &(nb, _) in &self.adj[u] {
                if !seen[nb] {
                    seen[nb] = true;
                    queue.push_back(nb);
                }
            }
        }
        best.map(|(i, _)| i)
    }

    /// A* path from node `start` to `goal` (node indices), or `None` if unreachable.
    pub fn path(&self, start: usize, goal: usize) -> Option<Vec<usize>> {
        self.path_inner(start, goal, |_| 0.0)
    }

    /// A* path from `start` to `goal` with a per-node **additive cost overlay**
    /// added to each edge leaving a node (Plan 08 T3): edge cost `cur→nb` =
    /// `(base_cost + overlay[cur]).max(EPS)`. A positive overlay (danger) makes a
    /// node costly to leave → routes around it; a negative overlay (popularity)
    /// makes it cheap → gravitates toward it. Reachability is unchanged (the
    /// overlay never removes an edge), so this returns `None` iff [`Self::path`].
    ///
    /// The heuristic stays Euclidean distance (a base-cost lower bound); with a
    /// negative overlay it may be inadmissible, yielding a slightly suboptimal
    /// path — acceptable, since we *want* the popularity bias to show through.
    pub fn path_weighted(&self, start: usize, goal: usize, overlay: &[f32]) -> Option<Vec<usize>> {
        self.path_inner(start, goal, |cur| overlay.get(cur).copied().unwrap_or(0.0))
    }

    /// A* path from `start` to `goal`, excluding (heavily penalising) any node in
    /// `blacklist`. Used by the navigation driver's give-up recovery: when a bot
    /// repeatedly fails to traverse a waypoint, that node is blacklisted so the next
    /// plan routes around it rather than retrying the same path.
    pub fn path_excluding(
        &self,
        start: usize,
        goal: usize,
        blacklist: &HashSet<usize>,
    ) -> Option<Vec<usize>> {
        if blacklist.is_empty() {
            return self.path(start, goal);
        }
        const PENALTY: f32 = 1_000_000.0;
        self.path_inner(start, goal, |cur| {
            if blacklist.contains(&cur) {
                PENALTY
            } else {
                0.0
            }
        })
    }

    /// Like `path_excluding` but also penalises specific directed edges `(from, to)`.
    /// Node blacklist and edge blacklist are applied independently and additively.
    /// Used by the fell-off-ledge recovery to avoid the exact staircase approach
    /// that caused a fall without blocking all routes through those nodes.
    pub fn path_excluding_edges(
        &self,
        start: usize,
        goal: usize,
        node_bl: &HashSet<usize>,
        edge_bl: &HashSet<(usize, usize)>,
    ) -> Option<Vec<usize>> {
        if node_bl.is_empty() && edge_bl.is_empty() {
            return self.path(start, goal);
        }
        const PENALTY: f32 = 1_000_000.0;
        if start == goal {
            return Some(vec![start]);
        }
        let n = self.nodes.len();
        let mut g = vec![f32::INFINITY; n];
        let mut came: Vec<Option<usize>> = vec![None; n];
        let mut closed = vec![false; n];
        g[start] = 0.0;
        let mut open: BinaryHeap<Reverse<(FOrd, usize)>> = BinaryHeap::new();
        open.push(Reverse((
            FOrd(dist(&self.nodes[start], &self.nodes[goal])),
            start,
        )));

        while let Some(Reverse((_, cur))) = open.pop() {
            if cur == goal {
                return Some(reconstruct(&came, start, goal));
            }
            if closed[cur] {
                continue;
            }
            closed[cur] = true;
            let node_cost = if node_bl.contains(&cur) { PENALTY } else { 0.0 };
            for &(nb, cost) in &self.adj[cur] {
                if closed[nb] {
                    continue;
                }
                let edge_cost = if edge_bl.contains(&(cur, nb)) {
                    PENALTY
                } else {
                    0.0
                };
                let ng = g[cur] + (cost + node_cost + edge_cost).max(EPS);
                if ng < g[nb] {
                    g[nb] = ng;
                    came[nb] = Some(cur);
                    let f = ng + dist(&self.nodes[nb], &self.nodes[goal]);
                    open.push(Reverse((FOrd(f), nb)));
                }
            }
        }
        None
    }

    /// String-pull smoothing (Plan 14 T1): collapse a grid path into the longest
    /// legal straight runs. From `from` (current bot position), scan forward through
    /// `path`; for each node the LOS trace is clear, keep extending; the first
    /// blocked candidate commits the last-visible node as the new apex. Repeat until
    /// the goal is reached. Returns a new node-index sequence (subset of `path`).
    ///
    /// Uses a zero-size (point) trace — individual edges already have hull clearance
    /// from `generate()`; the string-pull only checks whether the intermediate nodes
    /// can be skipped (line-of-sight), which the half-space/BSP visibility model handles
    /// exactly. This is the "Simple Stupid Funnel Algorithm" via successive LOS tests
    /// (distilled §9).
    pub fn smooth_path(&self, cm: &CollisionModel, path: &[usize], from: [f32; 3]) -> Vec<usize> {
        if path.len() <= 2 {
            return path.to_vec();
        }

        // Maximum allowed Z-delta when collapsing an intermediate node.
        //
        // A zero-size point trace between staircase landings clears solid geometry
        // (the staircase interior is open air), so naive smoothing eliminates all
        // intermediate stair nodes and leaves a diagonal path that exits the staircase
        // edge and causes the bot to walk off into free-fall.  Capping dz preserves
        // staircase sequences: each stair step is ~18 u (STEP), so 2.5 steps ≈ 48 u
        // is safely above noise but well below a real staircase run (~136 u for a
        // multi-floor flight).  Ramps on flat terrain accumulate < 24 u per node, so
        // gentle slopes are still smoothable.
        const MAX_SMOOTH_DZ: f32 = 48.0;
        // Cap horizontal lookahead: point traces clear staircase interiors and
        // platform voids (the Z-delta cap handles stairs; the hdist cap limits
        // how far the bot races before it can react to an edge).  5 grid cells
        // (120u) is enough to shortcut local wall corners without sending the bot
        // 600u across an open platform toward a distant waypoint it may overshoot.
        const MAX_SMOOTH_HDIST: f32 = 120.0;

        let mut result = Vec::new();
        result.push(path[0]);

        let mut apex = from;
        let mut commit_idx = 0usize; // index in `path` we most recently committed

        // Smoothing LOS hull: full bot WIDTH/DEPTH (±16) so a shortcut that would
        // clip the bot's body on an inside corner is rejected (a zero-width point
        // trace clears such corners and causes wall-bumping). Ceiling headroom is
        // reduced from +32 to +24 to avoid spurious `startsolid` at high nodes whose
        // hull top exactly touches a low ceiling (e.g. q2dm1 z=920 node under z=952
        // ceiling) — that edge case previously made the team disable hull smoothing.
        const SMOOTH_MINS: [f32; 3] = [-16.0, -16.0, -24.0];
        const SMOOTH_MAXS: [f32; 3] = [16.0, 16.0, 24.0];
        while commit_idx < path.len() - 1 {
            // Scan forward from apex for the furthest LOS-clear node.
            let mut furthest = commit_idx;
            for (j, &node_idx) in path.iter().enumerate().skip(commit_idx + 1) {
                let candidate = self.nodes[node_idx];
                // Don't smooth across significant elevation changes: the trace
                // may clear staircase interiors even though the bot can't walk there
                // directly (it needs the actual stair geometry underfoot).
                if (candidate[2] - apex[2]).abs() > MAX_SMOOTH_DZ {
                    break;
                }
                // Cap horizontal range: a bot aiming for a far node can overshoot
                // ledge edges at full speed before it can correct course.
                let hdist2 = (candidate[0] - apex[0]).powi(2) + (candidate[1] - apex[1]).powi(2);
                if hdist2 > MAX_SMOOTH_HDIST * MAX_SMOOTH_HDIST {
                    break;
                }
                let t = cm.trace(&apex, &candidate, &SMOOTH_MINS, &SMOOTH_MAXS, MASK_SOLID);
                if t.fraction >= 1.0 && !t.startsolid {
                    furthest = j;
                } else {
                    break; // first block — stop scanning
                }
            }

            if furthest > commit_idx {
                // Commit the furthest visible node.
                if result.last().copied() != Some(path[furthest]) {
                    result.push(path[furthest]);
                }
                apex = self.nodes[path[furthest]];
                commit_idx = furthest;
            } else {
                // Even the immediate next node is not LOS-clear — step past it.
                let next = commit_idx + 1;
                if result.last().copied() != Some(path[next]) {
                    result.push(path[next]);
                }
                apex = self.nodes[path[next]];
                commit_idx = next;
            }
        }

        result
    }

    /// Seed nav nodes at DM spawn origins (Plan 14 T3). For each spawn position
    /// that has no existing node within `STEP`, validate the position is walkable
    /// (hull not startsolid), add it, and hull-trace-connect it to nearby nodes
    /// (within ~128 u, same height-step constraint as `generate`). Returns the
    /// number of nodes added.
    pub fn seed_spawns(&mut self, cm: &CollisionModel, spawns: &[[f32; 3]]) -> usize {
        let mut to_add: Vec<[f32; 3]> = Vec::new();
        for &sp in spawns {
            // Skip if an existing node (or already-queued spawn) is close enough.
            let already = self.nodes.iter().chain(to_add.iter()).any(|n| {
                let dx = n[0] - sp[0];
                let dy = n[1] - sp[1];
                let dz = n[2] - sp[2];
                dx * dx + dy * dy + dz * dz < STEP * STEP
            });
            if already {
                continue;
            }
            // Validate the spawn position is not embedded in solid geometry.
            let stand = cm.trace(&sp, &sp, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
            if stand.startsolid {
                continue;
            }
            to_add.push(sp);
        }

        // seed nodes use a tighter dz cap than STAIR_MAX: only connect to nodes
        // within 3 stair steps (3×STEP=54u) of the seed position. Cross-floor
        // connections (e.g. weapon node at z=912 to a platform at z=792) are
        // invalid — the bot cannot walk 100+ units straight up — and the main
        // graph (generate + bridge_components) already handles real staircase
        // connections between floors.
        const SEED_MAX_DZ: f32 = STEP * 3.0; // 54 u ≈ 3 stair steps
        let added = to_add.len();
        for wp in to_add {
            let new_idx = self.nodes.len();
            self.nodes.push(wp);
            self.adj.push(Vec::new());
            // Connect to all existing nodes (pre-this-addition) within reach.
            for other_idx in 0..new_idx {
                let other = self.nodes[other_idx];
                let dz = wp[2] - other[2];
                if dz.abs() > SEED_MAX_DZ {
                    continue;
                }
                let d2 = {
                    let dx = wp[0] - other[0];
                    let dy = wp[1] - other[1];
                    dx * dx + dy * dy
                };
                if d2 > 128.0 * 128.0 {
                    continue;
                }
                let connected = if dz.abs() <= STEP {
                    let t = cm.trace(&wp, &other, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                    if !t.startsolid && t.fraction >= 1.0 {
                        true
                    } else if dz.abs() > 0.5 {
                        let (lower, upper) = if dz < 0.0 { (wp, other) } else { (other, wp) };
                        walkable_stair(cm, lower, upper)
                    } else {
                        false
                    }
                } else {
                    let (lower, upper) = if dz < 0.0 { (wp, other) } else { (other, wp) };
                    walkable_stair(cm, lower, upper)
                };
                if !connected {
                    continue;
                }
                let cost = dist(&wp, &other);
                self.adj[new_idx].push((other_idx, cost));
                self.adj[other_idx].push((new_idx, cost));
            }
        }
        added
    }

    /// Connect an already-added node at `idx` to all OTHER nodes in the graph that
    /// are within `max_hdist` horizontally and `STAIR_MAX` vertically, using the
    /// same hull/stair trace logic as `generate`. Bidirectional edges are added.
    /// Returns the number of new edges added. Used to wire elevator (func_plat) nodes
    /// into the existing walkable graph after the main generation pass.
    pub fn connect_node_to_nearby(
        &mut self,
        cm: &CollisionModel,
        idx: usize,
        max_hdist: f32,
    ) -> usize {
        let wp = self.nodes[idx];
        let mut added = 0;
        let n = self.nodes.len();
        for other_idx in 0..n {
            if other_idx == idx {
                continue;
            }
            let other = self.nodes[other_idx];
            let dz = wp[2] - other[2];
            if dz.abs() > STAIR_MAX {
                continue;
            }
            let dx = wp[0] - other[0];
            let dy = wp[1] - other[1];
            if dx * dx + dy * dy > max_hdist * max_hdist {
                continue;
            }
            if self.adj[idx].iter().any(|&(nb, _)| nb == other_idx) {
                continue; // already connected
            }
            let connected = if dz.abs() <= STEP {
                let t = cm.trace(&wp, &other, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                if !t.startsolid && t.fraction >= 1.0 {
                    true
                } else if dz.abs() > 0.5 {
                    let (lower, upper) = if dz > 0.0 { (other, wp) } else { (wp, other) };
                    walkable_stair(cm, lower, upper)
                } else {
                    false
                }
            } else {
                let (lower, upper) = if dz > 0.0 { (other, wp) } else { (wp, other) };
                walkable_stair(cm, lower, upper)
            };
            if connected {
                let cost = dist(&wp, &other);
                self.adj[idx].push((other_idx, cost));
                self.adj[other_idx].push((idx, cost));
                added += 1;
            }
        }
        added
    }

    /// Stitch together fragments of the nav graph that are *physically* walkable into
    /// each other but were left in separate connected components because grid sampling
    /// dropped the threshold column between them (a doorway, a 1–2 cell gap, a step
    /// where the multi-floor probe landed on a different z per column).
    ///
    /// The strict 8-neighbour edge-builder in [`Self::generate`] only links nodes in
    /// adjacent grid cells (`dh <= ~34 u`). When two walkable regions are sampled but
    /// the cells exactly on their shared border aren't, the regions never connect even
    /// though a player walks straight across. This pass looks for the *closest* node
    /// pair between two different components within `max_hdist` horizontally and
    /// `STAIR_MAX` vertically, and adds an edge iff `walkable_link_bridge` confirms a
    /// clear hull/stair path. The trace guard makes it impossible to bridge across a wall,
    /// ceiling, or floor (e.g. the unreachable roof component stays isolated).
    ///
    /// Runs repeatedly to a fixed point: bridging A↔B can expose a now-reachable C.
    /// Uses a spatial hash bucketed at `max_hdist` so it stays near-linear, not O(n²).
    /// Returns the total number of bridge edges added across all iterations.
    pub fn bridge_components(&mut self, cm: &CollisionModel, max_hdist: f32) -> usize {
        let mut total = 0;
        // A handful of iterations reaches a fixed point on real maps; cap to be safe.
        for _ in 0..8 {
            let added = self.bridge_pass(cm, max_hdist);
            total += added;
            if added == 0 {
                break;
            }
        }
        total
    }

    /// One sweep of [`Self::bridge_components`]. Computes current components, then for
    /// every node tries to connect it to the nearest node in a *different* component
    /// within range. Returns the number of edges added this pass.
    fn bridge_pass(&mut self, cm: &CollisionModel, max_hdist: f32) -> usize {
        let n = self.nodes.len();
        if n == 0 {
            return 0;
        }

        // node → component id (so we only bridge across components, never within).
        let comps = self.components();
        let mut comp_id = vec![usize::MAX; n];
        for (ci, c) in comps.iter().enumerate() {
            for &node in c {
                comp_id[node] = ci;
            }
        }
        if comps.len() <= 1 {
            return 0; // already fully connected
        }

        // Spatial hash bucketed at `max_hdist` so each node only examines the 3×3
        // block of cells around it (any node within max_hdist lives in one of them).
        let cell = max_hdist.max(1.0);
        let key = |p: &[f32; 3]| ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32);
        let mut buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, p) in self.nodes.iter().enumerate() {
            buckets.entry(key(p)).or_default().push(i);
        }

        // Enumerate every cross-component pair within range and test walkability.
        // Using `j > i` enumerates each unordered pair exactly once. The expensive part is
        // `walkable_stair_link_orig` (cm traces); it is read-only, so the per-`i` candidate
        // search runs in PARALLEL across cores. `flat_map_iter` preserves order, so the
        // collected candidate list (and the sequential apply below) is identical to a serial
        // pass. The component ids + spatial buckets are read-only here.
        let max_h2 = max_hdist * max_hdist;
        let nodes = &self.nodes;
        let comp_id = &comp_id;
        let buckets = &buckets;
        let candidates: Vec<(usize, usize, f32)> = (0..n)
            .into_par_iter()
            .flat_map_iter(move |i| {
                let a = nodes[i];
                let (kx, ky) = key(&a);
                let mut local: Vec<(usize, usize, f32)> = Vec::new();
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        let Some(cellnodes) = buckets.get(&(kx + dx, ky + dy)) else {
                            continue;
                        };
                        for &j in cellnodes {
                            if j <= i || comp_id[j] == comp_id[i] {
                                continue; // each pair once; skip same-component
                            }
                            let b = nodes[j];
                            if (b[2] - a[2]).abs() > STAIR_MAX {
                                continue;
                            }
                            let h2 = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
                            if h2 > max_h2 {
                                continue;
                            }
                            if walkable_stair_link_orig(cm, a, b) {
                                local.push((i, j, dist(&a, &b)));
                            }
                        }
                    }
                }
                local.into_iter()
            })
            .collect();

        let mut added = 0;
        for (i, j, cost) in candidates {
            // Skip if a previous candidate this pass already linked these two.
            if self.adj[i].iter().any(|&(nb, _)| nb == j) {
                continue;
            }
            self.adj[i].push((j, cost));
            self.adj[j].push((i, cost));
            added += 1;
        }
        added
    }

    /// Bridge disconnected components via **vertical jump-down links** (Plan 42). Many
    /// q2dm3 floors connect only by dropping off a higher ledge onto a lower one (same XY,
    /// `dz` 56–256) — a move [`detect_jump_edges`] misses because its probe reaches only
    /// `spacing*1.5` (~36u). For every near-vertical cross-component node pair (horizontal
    /// `≤ max_hdist`, drop in `(STEP, max_fall]`) with a clear launch arc, add a
    /// `Jump{launch_yaw}` edge from the higher node to the lower one. Repeats up to
    /// `passes` times so a chain of floors fuses. Returns total jump edges added.
    ///
    /// One-directional (down) by design — Q2 lets you fall off a ledge but not jump back
    /// up the same height; the return route is a lift/stair the graph already has. Guarded
    /// by arc-clearance traces and a tight `max_hdist` (only genuine vertical drops), so it
    /// cannot manufacture a horizontal false bridge.
    pub fn bridge_components_via_jump(
        &mut self,
        cm: &CollisionModel,
        max_hdist: f32,
        max_fall: f32,
        passes: usize,
    ) -> usize {
        let zero = [0.0f32; 3];
        let mut total = 0;
        for _ in 0..passes {
            let n = self.nodes.len();
            let comps = self.components();
            if comps.len() <= 1 {
                break;
            }
            let mut comp_id = vec![usize::MAX; n];
            let mut comp_size = vec![0usize; comps.len()];
            for (ci, c) in comps.iter().enumerate() {
                comp_size[ci] = c.len();
                for &node in c {
                    comp_id[node] = ci;
                }
            }

            let cell = max_hdist.max(1.0);
            let key = |p: &[f32; 3]| ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32);
            let mut buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
            for (i, p) in self.nodes.iter().enumerate() {
                buckets.entry(key(p)).or_default().push(i);
            }

            // For each unordered cross-component pair within range, validate a jump-down from
            // the higher node onto the lower. Parallel read-only search (mirrors bridge_pass).
            let max_h2 = max_hdist * max_hdist;
            let nodes = &self.nodes;
            let comp_id_ref = &comp_id;
            let buckets_ref = &buckets;
            let candidates: Vec<(usize, usize, f32, f32)> = (0..n)
                .into_par_iter()
                .flat_map_iter(move |i| {
                    let a = nodes[i];
                    let (kx, ky) = key(&a);
                    let mut local: Vec<(usize, usize, f32, f32)> = Vec::new();
                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            let Some(cellnodes) = buckets_ref.get(&(kx + dx, ky + dy)) else {
                                continue;
                            };
                            for &j in cellnodes {
                                if j <= i || comp_id_ref[j] == comp_id_ref[i] {
                                    continue;
                                }
                                let b = nodes[j];
                                let h2 = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
                                if h2 > max_h2 {
                                    continue;
                                }
                                // Orient: hi = higher node, lo = lower.
                                let (hi, lo) = if a[2] >= b[2] { (i, j) } else { (j, i) };
                                if let Some((cost, yaw)) =
                                    jump_down_link(nodes, cm, &zero, max_fall, hi, lo)
                                {
                                    local.push((hi, lo, cost, yaw));
                                }
                            }
                        }
                    }
                    local.into_iter()
                })
                .collect();

            // Apply: shortest-drop candidates first, only if still cross-component (union-find
            // by re-checking comp membership as we add — keeps it a real bridge, avoids dupes).
            let mut cand = candidates;
            cand.sort_by(|x, y| x.2.partial_cmp(&y.2).unwrap());
            let mut uf = UnionFind::new(n);
            for (ci, c) in comps.iter().enumerate() {
                let _ = ci;
                let mut it = c.iter();
                if let Some(&first) = it.next() {
                    for &node in it {
                        uf.union(first, node);
                    }
                }
            }
            let mut added = 0;
            for (hi, lo, cost, yaw) in cand {
                if uf.find(hi) == uf.find(lo) {
                    continue; // already connected (this pass) → skip redundant
                }
                uf.union(hi, lo);
                self.adj[hi].push((lo, cost));
                self.jump_edges.insert((hi, lo));
                self.jump_yaws.insert((hi, lo), yaw);
                added += 1;
            }
            total += added;
            if added == 0 {
                break;
            }
        }
        total
    }

    /// Rescue pass for **stranded spawn-bearing components** (Plan 52). After
    /// [`Self::bridge_components_via_jump`], a component that still holds DM spawns but
    /// is not the play component gets one more chance: the same jump-down candidate
    /// search, but with a deeper `max_fall` and restricted to pairs linking the stranded
    /// component to the play component — spawnless ceiling/wall-top networks are never
    /// touched, so this cannot inflate the play area with junk surfaces.
    ///
    /// Motivation: base64's grenade-launcher room (spawn at `(-720,824,-520)`) exits via
    /// a ~288u floor-shaft drop — survivable in Q2 (minor fall damage) but past the
    /// universal `JUMP_BRIDGE_MAX_FALL` of 256 that q2dm* graphs are tuned to. Deep
    /// drops stay opt-in: this pass adds **one** shortest validated edge per stranded
    /// component (enough for the undirected connectivity gate and for A* to route the
    /// drop) and is a no-op on maps whose spawns already connect.
    pub fn rescue_stranded_spawns(
        &mut self,
        cm: &CollisionModel,
        max_hdist: f32,
        max_fall: f32,
        spawns: &[[f32; 3]],
    ) -> usize {
        let comps = self.components();
        if comps.len() <= 1 || spawns.is_empty() {
            return 0;
        }
        let mut comp_id = vec![usize::MAX; self.nodes.len()];
        for (ci, c) in comps.iter().enumerate() {
            for &node in c {
                comp_id[node] = ci;
            }
        }
        // Spawn count per component — the same nearest-node mapping the connectivity
        // gate uses, so "stranded" here is exactly what the gate would flag.
        let mut spawn_count = vec![0usize; comps.len()];
        for sp in spawns {
            if let Some(nd) = self.nearest(sp) {
                if comp_id[nd] != usize::MAX {
                    spawn_count[comp_id[nd]] += 1;
                }
            }
        }
        let Some(play) = (0..comps.len()).max_by_key(|&ci| (spawn_count[ci], comps[ci].len()))
        else {
            return 0;
        };
        let stranded: Vec<usize> = (0..comps.len())
            .filter(|&ci| ci != play && spawn_count[ci] > 0)
            .collect();
        if stranded.is_empty() {
            return 0;
        }
        for &ci in &stranded {
            tracing::debug!(
                comp = ci,
                nodes = comps[ci].len(),
                spawns = spawn_count[ci],
                "rescue: stranded spawn component"
            );
        }

        // Stranded components are small (a room, a ledge); iterate their nodes and scan
        // play-component nodes bucketed at `max_hdist` — cheaper than the full-graph
        // bucketing of the normal pass.
        let cell = max_hdist.max(1.0);
        let key = |p: &[f32; 3]| ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32);
        let mut play_buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for &i in &comps[play] {
            play_buckets.entry(key(&self.nodes[i])).or_default().push(i);
        }

        let zero = [0.0f32; 3];
        let max_h2 = max_hdist * max_hdist;
        // (hi, lo, cost, yaw, stranded component)
        let mut candidates: Vec<(usize, usize, f32, f32, usize)> = Vec::new();
        let mut in_range = 0usize;
        for &ci in &stranded {
            for &i in &comps[ci] {
                let a = self.nodes[i];
                let (kx, ky) = key(&a);
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        let Some(cellnodes) = play_buckets.get(&(kx + dx, ky + dy)) else {
                            continue;
                        };
                        for &j in cellnodes {
                            let b = self.nodes[j];
                            let h2 = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
                            if h2 > max_h2 {
                                continue;
                            }
                            let (hi, lo) = if a[2] >= b[2] { (i, j) } else { (j, i) };
                            let drop = self.nodes[hi][2] - self.nodes[lo][2];
                            if drop > STEP && drop <= max_fall {
                                in_range += 1;
                                tracing::trace!(
                                    hi = ?self.nodes[hi],
                                    lo = ?self.nodes[lo],
                                    drop,
                                    "rescue: geometric pair"
                                );
                            }
                            if let Some((cost, yaw)) =
                                jump_down_link(&self.nodes, cm, &zero, max_fall, hi, lo)
                            {
                                candidates.push((hi, lo, cost, yaw, ci));
                            }
                        }
                    }
                }
            }
        }
        tracing::debug!(
            in_range,
            candidates = candidates.len(),
            "rescue: validated jump-down candidates"
        );
        candidates.sort_by(|x, y| x.2.total_cmp(&y.2));
        let mut rescued_comps: HashSet<usize> = HashSet::new();
        let mut added = 0;
        for (hi, lo, cost, yaw, ci) in candidates {
            if !rescued_comps.insert(ci) {
                continue; // this stranded component already got its (shortest) edge
            }
            self.adj[hi].push((lo, cost));
            self.jump_edges.insert((hi, lo));
            self.jump_yaws.insert((hi, lo), yaw);
            added += 1;
        }
        added
    }

    /// The **play component**: the connected component containing the most DM spawn
    /// points. This is the area the game actually happens in — and the right set for
    /// bots to roam. It is NOT necessarily the component with the most *nodes*: large
    /// Q2 maps generate big spurious surface networks (perimeter wall-tops / ceilings
    /// sampled by the floor probe) that no player can reach. Those out-size the play
    /// area in node count but contain zero spawns. Spawn-presence is the true signal
    /// of "this is where players are," so we rank components by spawn count, breaking
    /// ties by node count. Returns the component's node indices (empty if no spawns).
    pub fn largest_spawn_component(&self, spawns: &[[f32; 3]]) -> Vec<usize> {
        let comps = self.components();
        if comps.is_empty() {
            return Vec::new();
        }
        let mut node_comp = vec![usize::MAX; self.nodes.len()];
        for (ci, c) in comps.iter().enumerate() {
            for &n in c {
                node_comp[n] = ci;
            }
        }
        let mut spawn_count = vec![0usize; comps.len()];
        for sp in spawns {
            if let Some(n) = self.nearest(sp) {
                let ci = node_comp[n];
                if ci != usize::MAX {
                    spawn_count[ci] += 1;
                }
            }
        }
        // Pick the component with the most spawns; tie-break on node count (comps is
        // already sorted by node count desc, so the first max wins the tie).
        let best = (0..comps.len()).max_by_key(|&ci| (spawn_count[ci], comps[ci].len()));
        best.map(|ci| comps[ci].clone()).unwrap_or_default()
    }

    /// Check how many of the given spawn positions are in the **play component**
    /// (the largest spawn-bearing component, see [`Self::largest_spawn_component`]).
    /// Returns `(in_play_component, total)`. When every spawn is mutually reachable
    /// this is `(total, total)` — the contract `check_spawn_connectivity` enforces.
    pub fn spawns_in_largest_component(&self, spawns: &[[f32; 3]]) -> (usize, usize) {
        if spawns.is_empty() {
            return (0, 0);
        }
        let play: HashSet<usize> = self.largest_spawn_component(spawns).into_iter().collect();
        let connected = spawns
            .iter()
            .filter(|sp| self.nearest(sp).is_some_and(|i| play.contains(&i)))
            .count();
        (connected, spawns.len())
    }

    /// Detect ledge-drop jump links (Plan 14 T2). Conservative: downward jumps only
    /// (height drop > STEP, < MAX_FALL=256). For each node A, probes in 8 directions
    /// at `spacing * 1.5`; if a lower floor is found AND the nearest sampled node B
    /// is within `STEP * 2` of the landing point AND no walk-edge already exists,
    /// adds a `Jump { launch_yaw }` edge A→B and records it. Returns the count added.
    ///
    /// Call after `generate()` (+ optional `seed_spawns()`). The resulting edges are
    /// safe to traverse — Q2 ignores jump while airborne, so holding jump while
    /// approaching a ledge harmlessly fires once on landing.
    ///
    /// NOTE: Only creates DOWNWARD edges. Full connectivity requires `generate()` to
    /// find all walkable paths (stairs, ramps, etc.) through grid sampling.
    pub fn detect_jump_edges(&mut self, cm: &CollisionModel, spacing: f32) -> usize {
        const MAX_FALL: f32 = 256.0;
        const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
        const DIRS: [(f32, f32); 8] = [
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (D, D),
            (-D, D),
            (D, -D),
            (-D, -D),
        ];
        let zero = [0.0f32; 3];
        let probe_dist = spacing * 1.5;
        let n = self.nodes.len();
        let graph: &NavGraph = self;

        // Phase 1 (PARALLEL): per node, find the ledge-drop jump edges to add. Read-only —
        // each probe is independent cm.trace + nearest() lookups (nearest is an O(n) scan,
        // so this whole pass is O(n²) and the dominant build cost — parallelising it across
        // cores is the big win). Returns per-node lists of (b, cost, launch_yaw), deduped by
        // b within the node (matching the original's "skip if already added this node").
        let per_node: Vec<Vec<(usize, f32, f32)>> = (0..n)
            .into_par_iter()
            .map(|a| {
                let an = graph.nodes[a];
                let mut found: Vec<(usize, f32, f32)> = Vec::new();
                for (dx, dy) in DIRS {
                    let px = an[0] + dx * probe_dist;
                    let py = an[1] + dy * probe_dist;
                    // Down-trace from above the probe to A - MAX_FALL.
                    let top = [px, py, an[2] + 200.0];
                    let bot = [px, py, an[2] - MAX_FALL];
                    let down = cm.trace(&top, &bot, &zero, &zero, MASK_SOLID);
                    if down.fraction >= 1.0 || down.startsolid {
                        continue; // no floor below
                    }
                    let floor_z = down.endpos[2];
                    let drop = an[2] - floor_z;
                    if !(STEP..=MAX_FALL).contains(&drop) {
                        continue; // not a meaningful downward ledge
                    }
                    // The probe's MASK_SOLID floor may be a lava BED, and the bot lands
                    // at the probe point (launch_yaw aims there) skidding onward — reject
                    // ledge drops whose landing strip is deadly (Plan 50 E3).
                    if landing_strip_deadly(cm, [px, py, floor_z + 24.0], [dx, dy]) {
                        continue;
                    }
                    let landing = [px, py, floor_z + 24.0];
                    let Some(b) = graph.nearest(&landing) else {
                        continue;
                    };
                    let bn = graph.nodes[b];
                    if b == a {
                        continue;
                    }
                    // B must be close to landing horizontally.
                    let dx2 = (bn[0] - landing[0]).powi(2) + (bn[1] - landing[1]).powi(2);
                    if dx2 > (STEP * 2.0).powi(2) {
                        continue;
                    }
                    // Skip if a walk edge already exists, or we already picked this b for a.
                    if graph.adj[a].iter().any(|&(nb, _)| nb == b)
                        || found.iter().any(|&(bb, _, _)| bb == b)
                    {
                        continue;
                    }
                    // Ensure the first half of the path A→B is clear (no wall).
                    let t = cm.trace(&an, &bn, &zero, &zero, MASK_SOLID);
                    if t.startsolid || t.fraction < 0.4 {
                        continue;
                    }
                    let launch_yaw = dy.atan2(dx).to_degrees();
                    found.push((b, dist(&an, &bn), launch_yaw));
                }
                found
            })
            .collect();

        // Phase 2 (SEQUENTIAL): apply. Each node has a distinct `a`, so jump-edge keys (a, b)
        // never collide across nodes — the result equals the original in-place version.
        let mut added = 0;
        for (a, list) in per_node.into_iter().enumerate() {
            for (b, cost, launch_yaw) in list {
                self.adj[a].push((b, cost));
                self.jump_edges.insert((a, b));
                self.jump_yaws.insert((a, b), launch_yaw);
                added += 1;
            }
        }
        added
    }

    /// Returns the [`EdgeKind`] of edge `(from, to)`. Returns `Walk` if the
    /// edge is not in the jump-edge set (or doesn't exist).
    pub fn edge_kind(&self, from: usize, to: usize) -> EdgeKind {
        if let Some(&launch_yaw) = self.jump_yaws.get(&(from, to)) {
            EdgeKind::Jump { launch_yaw }
        } else if self.swim_edges.contains(&(from, to)) {
            EdgeKind::Swim
        } else if self.ride_edges.contains(&(from, to)) {
            EdgeKind::Ride
        } else {
            EdgeKind::Walk
        }
    }

    /// Sum of base edge costs along a node path (diagnostics / degeneracy check).
    pub fn path_len(&self, path: &[usize]) -> f32 {
        path.windows(2)
            .map(|w| dist(&self.nodes[w[0]], &self.nodes[w[1]]))
            .sum()
    }

    /// A* core shared by [`Self::path`] and [`Self::path_weighted`]. `add(cur)`
    /// is the per-source-node additive overlay for the edge leaving `cur`
    /// (0.0 for the unweighted path).
    fn path_inner<F: Fn(usize) -> f32>(
        &self,
        start: usize,
        goal: usize,
        add: F,
    ) -> Option<Vec<usize>> {
        if start == goal {
            return Some(vec![start]);
        }
        let n = self.nodes.len();
        let mut g = vec![f32::INFINITY; n];
        let mut came = vec![None; n];
        let mut closed = vec![false; n];
        g[start] = 0.0;
        let mut open: BinaryHeap<Reverse<(FOrd, usize)>> = BinaryHeap::new();
        open.push(Reverse((
            FOrd(dist(&self.nodes[start], &self.nodes[goal])),
            start,
        )));

        while let Some(Reverse((_, cur))) = open.pop() {
            if cur == goal {
                return Some(reconstruct(&came, start, goal));
            }
            if closed[cur] {
                continue;
            }
            closed[cur] = true;
            let overlay = add(cur);
            for &(nb, cost) in &self.adj[cur] {
                if closed[nb] {
                    continue;
                }
                let ng = g[cur] + (cost + overlay).max(EPS);
                if ng < g[nb] {
                    g[nb] = ng;
                    came[nb] = Some(cur);
                    // Penalise nodes that are below the goal height: being
                    // 1u lower than the goal adds 0.5u to the heuristic.
                    // This discourages A* from routing DOWN when the goal is
                    // above the current node (e.g. weapon at z=920 — the
                    // Euclidean-only heuristic incorrectly prefers z=472 nodes
                    // that are "closer" in 3D but require a longer actual path).
                    // Makes the heuristic inadmissible but prevents catastrophic
                    // down-then-up detours that cost 30-40 s of extra travel.
                    let goal_z = self.nodes[goal][2];
                    let nb_z = self.nodes[nb][2];
                    // Factor ≥2.5 is required: at z=920→weapon the horizontal
                    // distance savings (1237u→591u) for a z=664 detour outweigh
                    // a 0.5 factor (128u), so A* still routes down. 3.0 tips
                    // the balance: 256u drop × 3.0 = 768u > the 646u h-saving.
                    let vpen = (goal_z - nb_z).max(0.0) * 3.0;
                    let f = ng + dist(&self.nodes[nb], &self.nodes[goal]) + vpen;
                    open.push(Reverse((FOrd(f), nb)));
                }
            }
        }
        None
    }
}

/// Sample ALL walkable floors at grid column (x, y) by probing downward repeatedly,
/// stepping through solid-brush layers to reach indoor floors.
///
/// The original single-probe approach only found the topmost floor (e.g. a roof),
/// missing any indoor levels beneath a solid ceiling. Multi-floor probing is the
/// fix: after each surface hit, walk downward through the solid brush in 8-unit
/// steps until re-entering empty space, then probe again for the next floor.
///
/// Safe upper bound: Q2 maps never have more than 8 walkable floor levels per column
/// (typical is 1-2). Each step through a brush takes at most O(thickness / 8) iters
/// (~16 for a 128-unit thick ceiling), so the per-column cost stays small.
fn floor_waypoints_multi(
    cm: &CollisionModel,
    x: f32,
    y: f32,
    bounds: ([f32; 3], [f32; 3]),
) -> Vec<[f32; 3]> {
    const MAX_FLOORS: usize = 8;
    let floor_min_z = bounds.0[2] - 200.0;
    let mut results = Vec::new();
    let mut probe_z = bounds.1[2] + 200.0;

    for _ in 0..MAX_FLOORS {
        let top = [x, y, probe_z];
        let bot = [x, y, floor_min_z];
        let down = cm.trace(&top, &bot, &[0.0; 3], &[0.0; 3], MASK_SOLID);

        if down.fraction >= 1.0 || down.startsolid {
            break; // no more floors in this column
        }

        let floor_z = down.endpos[2];
        // bot origin stands 24 u above the floor (hull mins.z = -24)
        let wp = [x, y, floor_z + 24.0];

        // A lava/slime-covered floor is never a node: the `wp` water check below only
        // rejects liquid deeper than 24 u, so a SHALLOW pool would otherwise place a
        // "dry" node hovering over lava (Plan 48 L1).
        if !floor_is_deadly(cm, &down.endpos) && cm.point_contents(&wp) & MASK_WATER == 0 {
            let stand = cm.trace(&wp, &wp, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
            if !stand.startsolid {
                results.push(wp);
            } else if let Some(rest) = hull_rest_z(cm, x, y, floor_z) {
                // Hull-rest fallback (Plan 52): on non-flat floors (V-grooves, 45°
                // channel beds, sloped trim) the POINT trace reaches deeper than the
                // 32×32 hull can — the hull straddles the slopes and comes to rest
                // higher, exactly like pmove does for a real player. base64's drain
                // duct (the only exit from its GL-room spawn) is such a groove and
                // sampled ZERO nodes without this.
                results.push([x, y, rest]);
            }
        }

        // Step downward through the solid brush we just hit to find the next
        // empty cavity below. Steps of 8 u handle the thinnest realistic Q2 brush.
        let mut exit_z = floor_z - 8.0;
        let mut found_next = false;
        while exit_z > floor_min_z {
            if cm.point_contents(&[x, y, exit_z]) & MASK_SOLID == 0 {
                probe_z = exit_z;
                found_next = true;
                break;
            }
            exit_z -= 8.0;
        }
        if !found_next {
            break;
        }
    }

    results
}

/// Where the player hull actually comes to rest in column `(x, y)` above a point-floor
/// at `floor_z` (Plan 52). Returns the standing **origin** z, or `None` if the hull
/// cannot rest cleanly within `HULL_REST_MAX` above the point contact.
///
/// The floor probe's point trace finds the deepest solid contact, but on a non-flat
/// floor (V-groove, sloped channel bed) the 32×32 hull bridges the slopes and rests
/// higher — the stationary hull check at `point + 24` is `startsolid` even though a
/// real player stands there fine (pmove resolves the same way this trace does).
fn hull_rest_z(cm: &CollisionModel, x: f32, y: f32, floor_z: f32) -> Option<f32> {
    // How far above the flat-floor origin (`floor_z + 24`) the hull may rest and still
    // count as "standing on this floor". A 45° groove that fits the 32u hull rests at
    // most ~16u higher than the groove bottom; 40 leaves margin without accepting a
    // rest on some unrelated rim above.
    const HULL_REST_MAX: f32 = 40.0;
    let top = [x, y, floor_z + 24.0 + HULL_REST_MAX];
    let bot = [x, y, floor_z + 24.0];
    let down = cm.trace(&top, &bot, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
    if down.startsolid {
        return None; // no clearance even at the top of the window
    }
    if down.fraction >= 1.0 {
        return None; // hull never contacts — the startsolid came from something else
    }
    let rest = down.endpos[2];
    // Confirm the rest position is genuinely clear (paranoia: zero-length hull check,
    // same predicate the flat-floor path uses).
    let stand = cm.trace(
        &[x, y, rest],
        &[x, y, rest],
        &HULL_MINS,
        &HULL_MAXS,
        MASK_SOLID,
    );
    if stand.startsolid {
        return None;
    }
    Some(rest)
}

/// Sample submerged + surface swim nodes in column `(x, y)` (Plan 39).
///
/// Walks the column top-down looking for contiguous `CONTENTS_WATER` spans (lava/slime
/// are deadly and never swum, so only `CONTENTS_WATER` qualifies). For each span emits a
/// **surface** node near the water top plus **submerged** lattice nodes every
/// [`SWIM_SPACING`] down to just above the pool floor. Each candidate is validated with a
/// reduced-hull point trace ([`SWIM_HULL_MINS`]/[`SWIM_HULL_MAXS`]) so positions embedded in
/// solid (tunnel walls, the pool floor) are skipped. Returns the bot-origin positions.
fn water_waypoints_multi(
    cm: &CollisionModel,
    x: f32,
    y: f32,
    bounds: ([f32; 3], [f32; 3]),
) -> Vec<[f32; 3]> {
    // Coarse vertical scan step to locate water spans; fine enough not to miss a thin pool.
    const SCAN: f32 = 16.0;
    let z_lo_bound = bounds.0[2] - 8.0;
    let z_hi_bound = bounds.1[2] + 8.0;

    // Collect contiguous water spans as (z_bottom, z_top) scanning downward.
    let mut spans: Vec<(f32, f32)> = Vec::new();
    let mut z = z_hi_bound;
    let mut span_top: Option<f32> = None;
    let mut last_water_z = z;
    while z >= z_lo_bound {
        let in_water = cm.point_contents(&[x, y, z]) & CONTENTS_WATER != 0;
        match (in_water, span_top) {
            (true, None) => {
                span_top = Some(z);
                last_water_z = z;
            }
            (true, Some(_)) => last_water_z = z,
            (false, Some(top)) => {
                spans.push((last_water_z, top));
                span_top = None;
            }
            (false, None) => {}
        }
        z -= SCAN;
    }
    if let Some(top) = span_top {
        spans.push((last_water_z, top));
    }

    let mut results = Vec::new();
    for (z_bot, z_top) in spans {
        // Surface node at the highest sampled water Z (a floating bot maps here). Then a
        // submerged lattice down to the pool floor. `validate` skips solid-embedded points.
        let mut zc = z_top;
        let mut pushed_bottom = false;
        while zc >= z_bot {
            push_water_node(cm, x, y, zc, &mut results);
            if (zc - z_bot).abs() < 1.0 {
                pushed_bottom = true;
            }
            zc -= SWIM_SPACING;
        }
        // Ensure a node near the pool floor even if the lattice stepped past it.
        if !pushed_bottom {
            push_water_node(cm, x, y, z_bot, &mut results);
        }
    }
    results
}

/// Try to connect two nodes with a **swim** edge (Plan 39 T3/T4). At least one endpoint is
/// a water node. Returns the edge cost if a reduced-hull 3-D trace is clear, else `None`.
///
/// Unlike walk edges, swim edges have **no STEP/STAIR gate** — a submerged bot moves freely
/// in 3-D — but `|dz|` is capped at [`WATER_VLINK`] so a single edge can't span a whole pool.
/// `both_water` (swim↔swim) scales cost by [`SWIM_COST_FACTOR`] (slow water move); a dry
/// endpoint (entry/exit) keeps the raw distance — walking/falling in/out is cheap.
fn try_swim_edge(cm: &CollisionModel, a: &[f32; 3], b: &[f32; 3], both_water: bool) -> Option<f32> {
    if (b[2] - a[2]).abs() > WATER_VLINK {
        return None;
    }
    let t = cm.trace(a, b, &SWIM_HULL_MINS, &SWIM_HULL_MAXS, MASK_SOLID);
    if t.startsolid || t.fraction < 1.0 {
        return None;
    }
    let d = dist(a, b);
    Some(if both_water { d * SWIM_COST_FACTOR } else { d })
}

/// Validate a candidate swim node at `(x, y, z)` and push it to `results` if it is inside
/// water and not embedded in solid (reduced-hull point trace). Deduplicates near-identical
/// Z so the surface/lattice/floor passes don't emit overlapping nodes.
fn push_water_node(cm: &CollisionModel, x: f32, y: f32, z: f32, results: &mut Vec<[f32; 3]>) {
    let p = [x, y, z];
    if cm.point_contents(&p) & CONTENTS_WATER == 0 {
        return;
    }
    let stand = cm.trace(&p, &p, &SWIM_HULL_MINS, &SWIM_HULL_MAXS, MASK_SOLID);
    if stand.startsolid {
        return;
    }
    if results.last().is_some_and(|l| (l[2] - z).abs() < 1.0) {
        return;
    }
    results.push(p);
}

/// Check whether a bot can walk *upward* from `lower` to `upper` via a staircase,
/// for the case where the direct height difference is in `(STEP, STAIR_MAX]`.
/// Public so the `nav-debug` diagnostic command can call it directly.
///
/// A direct diagonal hull trace clips stair *risers* (the vertical walls between
/// treads) even on fully walkable stairs. This function instead simulates Q2
/// pmove's step-climb pattern: step up by `STEP` vertically, then advance
/// horizontally by the proportional XY distance, repeating until reaching `upper`.
/// Each sub-trace uses the full player hull, so actual walls and cliff faces still
/// True if there is continuous walkable floor under the straight segment `a → b`.
///
/// A horizontal hull/point trace clears across an **open gap** (a pit has nothing
/// to obstruct it), so a path-smoothing shortcut validated only by a forward trace
/// can route the bot across thin air → it falls. This samples points every ~16 u
/// along the segment and requires solid floor within `FLOOR_PROBE` below each one.
/// Z is interpolated linearly between the endpoints (both already constrained to
/// `MAX_SMOOTH_DZ`), so the probe tracks gentle ramps/steps but rejects voids.
pub fn segment_has_floor(cm: &CollisionModel, a: [f32; 3], b: [f32; 3]) -> bool {
    // How far below the interpolated path a floor may be before the straight line
    // is "walking off an edge". A real walkable shortcut keeps the floor within
    // step-down range of the line; a drop-off/pit has floor far below. 96 u tolerates
    // a tall single step plus slack but rejects a true fall to a lower platform.
    // A zero-width (point) probe at the path centreline avoids false "gap" hits from
    // a 32-wide box catching side-edges next to a narrow but valid walkway.
    const FLOOR_PROBE: f32 = 96.0;
    let zero = [0.0f32; 3];
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let hdist = (dx * dx + dy * dy).sqrt();
    let samples = (hdist / 16.0).ceil() as usize;
    if samples <= 1 {
        return true; // endpoints are nav nodes — already known to have floor
    }
    for i in 1..samples {
        let f = i as f32 / samples as f32;
        let p = [a[0] + dx * f, a[1] + dy * f, a[2] + (b[2] - a[2]) * f];
        // The path itself passes through a deadly volume → not a walkable shortcut.
        if cm.point_contents(&p) & (CONTENTS_LAVA | CONTENTS_SLIME) != 0 {
            return false;
        }
        let down = [p[0], p[1], p[2] - FLOOR_PROBE];
        let t = cm.trace(&p, &down, &zero, &zero, MASK_SOLID);
        // No floor within FLOOR_PROBE (fraction == 1.0) → gap under the shortcut.
        if t.fraction >= 1.0 && !t.startsolid {
            return false;
        }
        // MASK_SOLID sees through liquids: a shallow lava/slime pool's solid BED
        // registers as "floor" even though crossing it kills the bot. Only a floor
        // whose surface is breathable/walkable (or safe water) counts.
        if !t.startsolid && floor_is_deadly(cm, &t.endpos) {
            return false;
        }
    }
    true
}

/// True when the solid floor at `endpos` (a down-trace hit point) lies under lava or
/// slime — standing there is death, so callers must not treat it as walkable support.
fn floor_is_deadly(cm: &CollisionModel, endpos: &[f32; 3]) -> bool {
    let above = [endpos[0], endpos[1], endpos[2] + 1.0];
    cm.point_contents(&above) & (CONTENTS_LAVA | CONTENTS_SLIME) != 0
}

/// True if a jump/fall LANDING at `base` (foot/origin level) with horizontal travel
/// direction `dir` touches lava/slime anywhere on the 0–48 u overshoot strip (Plan 50 E3).
/// A bot arrives with momentum under 10 Hz control — it does not stop dead on the landing
/// point; if the strip it skids across hangs over a lava channel, the edge is a death trap.
/// Every soak-verified q2dm3 lava entry was a FALL (vz −240..−690) clustered on such
/// landings.
fn landing_strip_deadly(cm: &CollisionModel, base: [f32; 3], dir: [f32; 2]) -> bool {
    let zero = [0.0f32; 3];
    for d in [0.0f32, 16.0, 32.0, 48.0] {
        let p = [base[0] + dir[0] * d, base[1] + dir[1] * d, base[2] + 8.0];
        if cm.point_contents(&p) & (CONTENTS_LAVA | CONTENTS_SLIME) != 0 {
            return true;
        }
        let down = [p[0], p[1], p[2] - 72.0];
        let t = cm.trace(&p, &down, &zero, &zero, MASK_SOLID);
        if !t.startsolid && t.fraction < 1.0 && floor_is_deadly(cm, &t.endpos) {
            return true;
        }
    }
    false
}

/// block the path. Stair risers don't block the upward vertical traces, and the
/// horizontal traces at each stepped height clear any risers below that level.
///
/// `upper[2] > lower[2]` is assumed; `total_dz` must be in `(0, STAIR_MAX]`.
/// Classification of one undirected edge for `prune_long_blocked_edges`. Pure function of
/// (nodes, collision model) — computed in parallel, merged sequentially.
#[derive(Debug, Clone, Copy, PartialEq)]
enum EdgeClass {
    /// Hull-clear or a confirmed real stair — part of the trustworthy base graph.
    Trustworthy(usize, usize),
    /// Blocked + not a real stair (flat-blocked, long-blocked, or steep-non-stair). The
    /// `f32` is the squared horizontal span, used to sort candidates shortest-first.
    Candidate(usize, usize, f32),
}

/// Classify the undirected edge `(a, b)` for the prune. Returns `None` for `a >= b` so each
/// undirected edge is visited once. Read-only: safe to call concurrently across edges.
fn classify_prune_edge(
    nodes: &[[f32; 3]],
    cm: &CollisionModel,
    max_hd: f32,
    a: usize,
    b: usize,
) -> Option<EdgeClass> {
    if a >= b {
        return None; // visit each undirected edge once
    }
    let pa = nodes[a];
    let pb = nodes[b];
    let hd2 = (pb[0] - pa[0]).powi(2) + (pb[1] - pa[1]).powi(2);
    let dz = (pb[2] - pa[2]).abs();
    let t = cm.trace(&pa, &pb, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
    let blocked = t.startsolid || t.fraction < 0.999;
    if !blocked {
        return Some(EdgeClass::Trustworthy(a, b)); // hull-clear base edge
    }
    // Blocked straight line. A short steep edge MIGHT be a real staircase the bot climbs
    // via pmove stepping — confirm with walkable_stair. If that also fails (stair=NO) it is
    // a false cliff (e.g. q2dm1 weapon 3972→3978 dz=128 hd=96) that traps bots → candidate.
    if hd2 <= max_hd * max_hd && dz > STEP {
        let (lo, hi) = if pa[2] < pb[2] { (pa, pb) } else { (pb, pa) };
        if walkable_stair(cm, lo, hi) {
            return Some(EdgeClass::Trustworthy(a, b)); // real stair
        }
    }
    Some(EdgeClass::Candidate(a, b, hd2)) // flat/long/steep-non-stair false edge
}

pub fn walkable_stair(cm: &CollisionModel, lower: [f32; 3], upper: [f32; 3]) -> bool {
    let total_dz = upper[2] - lower[2];
    let steps = (total_dz / STEP).ceil() as usize;
    let mut pos = lower;
    for step in 0..steps {
        let frac = (step + 1) as f32 / steps as f32;
        let target_z = lower[2] + total_dz * frac;
        // 1. Step up vertically at current XY.
        let stepped = [pos[0], pos[1], target_z];
        let up_t = cm.trace(&pos, &stepped, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if up_t.startsolid || up_t.fraction < 1.0 {
            return false;
        }
        // 2. Move horizontally to the proportional XY fraction of the total path.
        let forward = [
            lower[0] + (upper[0] - lower[0]) * frac,
            lower[1] + (upper[1] - lower[1]) * frac,
            target_z,
        ];
        let h_t = cm.trace(&stepped, &forward, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if h_t.startsolid || h_t.fraction < 1.0 {
            return false;
        }
        // 3. Floor-existence check: a real staircase tread is within STEP*2 below the
        //    bot's current position.  A false "folded-staircase" shortcut through open
        //    staircase air has NO intermediate floor — the nearest floor is the lower
        //    endpoint, > STEP*2 below at any intermediate step.  Probing STEP*2 down
        //    (36u) is deep enough to find a tread (the bot's hull min is -24u, so a
        //    tread surface at hull_min below the origin is found at fraction ≈ 0.67)
        //    but shallow enough to miss the lower floor at this step height.
        let floor_probe = cm.trace(
            &forward,
            &[forward[0], forward[1], forward[2] - STEP * 2.0],
            &HULL_MINS,
            &HULL_MAXS,
            MASK_SOLID,
        );
        if floor_probe.startsolid || floor_probe.fraction >= 1.0 {
            // startsolid: bot is inside geometry (invalid step position).
            // fraction=1.0: no floor within STEP*2 below — bot would be floating.
            return false;
        }
        // The hull floor hit is MASK_SOLID-only — a "tread" that is really a lava/slime
        // bed must not validate the stair (Plan 50 E1). Feet sit at endpos + mins.z.
        let feet = [
            floor_probe.endpos[0],
            floor_probe.endpos[1],
            floor_probe.endpos[2] + HULL_MINS[2] + 1.0,
        ];
        if cm.point_contents(&feet) & (CONTENTS_LAVA | CONTENTS_SLIME) != 0 {
            return false;
        }
        pos = forward;
    }
    true
}

/// Can a bot walk between `a` and `b` for the purpose of bridging two disconnected
/// nav-graph components? Uses the same logic as the generate()-phase edge builder:
/// direct hull trace for small dz, `walkable_stair` (with floor-existence check) for
/// large dz. This is the only function called by `bridge_pass`.
fn walkable_stair_link_orig(cm: &CollisionModel, a: [f32; 3], b: [f32; 3]) -> bool {
    let dz = b[2] - a[2];
    if dz.abs() > STAIR_MAX {
        return false;
    }
    if dz.abs() <= STEP {
        // Same trench blindness as the generate()-phase flat edges (Plan 50 E1): a clear
        // hull trace over a lava gap must not bridge components across it.
        let fwd = cm.trace(&a, &b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if !fwd.startsolid && fwd.fraction >= 1.0 {
            return segment_has_floor(cm, a, b);
        }
        let rev = cm.trace(&b, &a, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if !rev.startsolid && rev.fraction >= 1.0 {
            return segment_has_floor(cm, a, b);
        }
        if dz.abs() <= 0.5 {
            return false;
        }
    }
    // Reject near-vertical edges for bridge_pass: real Q2 staircases have
    // hdist/dz ≈ 1.0+ (each tread ≈ 18u wide, each riser ≈ 18u tall).
    // A slope < 0.3 means almost straight up — not a real staircase, so reject.
    // Known-good NW staircase bridge: hdist=120, dz=102 → slope=1.18 (well above 0.3).
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let hdist = (dx * dx + dy * dy).sqrt();
    if hdist / dz.abs() < 0.3 {
        return false;
    }
    let (lower, upper) = if dz > 0.0 { (a, b) } else { (b, a) };
    walkable_stair(cm, lower, upper)
}

/// Validate a jump-down link from higher node `hi` onto lower node `lo` (Plan 42,
/// [`NavGraph::bridge_components_via_jump`]). Requires the drop `hi.z - lo.z` to lie in
/// `(STEP, max_fall]` and the launch arc to be clear: the bot moves horizontally off the
/// `hi` ledge to above `lo`, then falls onto it. Returns `(cost, launch_yaw)` if valid.
fn jump_down_link(
    nodes: &[[f32; 3]],
    cm: &CollisionModel,
    zero: &[f32; 3],
    max_fall: f32,
    hi: usize,
    lo: usize,
) -> Option<(f32, f32)> {
    let hp = nodes[hi];
    let lp = nodes[lo];
    let drop = hp[2] - lp[2];
    if drop <= STEP || drop > max_fall {
        return None;
    }
    // Launch over the gap to directly above the landing, then fall. Tried at TWO launch
    // heights (Plan 35 T3): standing height first, then hop height (+32u — well under the
    // 45u jump apex). Real ledges often have a lip/curb at the edge that blocks the flat
    // standing-height sweep even though a bot trivially hops it — q2dm6/q2dm7's stacked-floor
    // junctions were all rejected this way (the brains DO jump on jump edges, so the hop is
    // faithful to how the edge is actually traversed).
    for launch_dz in [0.0, 32.0] {
        let start = [hp[0], hp[1], hp[2] + launch_dz];
        let over = [lp[0], lp[1], hp[2] + launch_dz];
        // The hop itself must be clear (only matters for the raised launch).
        if launch_dz > 0.0 {
            let up = cm.trace(&hp, &start, zero, zero, MASK_SOLID);
            if up.startsolid || up.fraction < 1.0 {
                continue;
            }
        }
        let t1 = cm.trace(&start, &over, zero, zero, MASK_SOLID);
        if t1.startsolid || t1.fraction < 0.95 {
            continue; // wall between the ledge and the drop point at this height
        }
        // Fall straight down onto the landing node.
        let t2 = cm.trace(&over, &lp, zero, zero, MASK_SOLID);
        if t2.startsolid || t2.fraction < 0.95 {
            continue; // overhang / ceiling blocks the fall
        }
        // Landing-overshoot check (Plan 50 E3): reject bridges whose landing strip
        // touches lava/slime — the bot arrives with momentum and skids past the node.
        let travel = {
            let dx = lp[0] - hp[0];
            let dy = lp[1] - hp[1];
            let h = (dx * dx + dy * dy).sqrt();
            if h < 1.0 {
                [0.0, 0.0]
            } else {
                [dx / h, dy / h]
            }
        };
        if landing_strip_deadly(cm, lp, travel) {
            return None; // the landing strip is the same for both launch heights
        }
        let yaw = (lp[1] - hp[1]).atan2(lp[0] - hp[0]).to_degrees();
        return Some((dist(&hp, &lp), yaw));
    }
    None
}

fn dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    dist2(a, b).sqrt()
}
fn dist2(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

fn reconstruct(came: &[Option<usize>], start: usize, goal: usize) -> Vec<usize> {
    let mut path = vec![goal];
    let mut cur = goal;
    while cur != start {
        cur = came[cur].expect("path chain must reach start");
        path.push(cur);
    }
    path.reverse();
    path
}

// f32 ordering wrapper for the BinaryHeap (no NaN in our costs).
#[derive(Clone, Copy, PartialEq)]
struct FOrd(f32);
impl Eq for FOrd {}
impl PartialOrd for FOrd {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for FOrd {
    fn cmp(&self, o: &Self) -> Ordering {
        self.0.total_cmp(&o.0)
    }
}

/// Disjoint-set (union-find) with path compression + union by rank. Used by
/// [`NavGraph::prune_long_blocked_edges`] to keep load-bearing bridges while pruning
/// redundant false edges in a single near-linear pass.
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // Path compression.
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            Ordering::Less => self.parent[ra] = rb,
            Ordering::Greater => self.parent[rb] = ra,
            Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two waypoints on flat ground, connected; A* finds the direct path.
    #[test]
    fn path_between_neighbors() {
        let g = NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [64.0, 0.0, 0.0], [128.0, 0.0, 0.0]],
            vec![vec![(1, 64.0)], vec![(0, 64.0), (2, 64.0)], vec![(1, 64.0)]],
        );
        let path = g.path(0, 2).unwrap();
        assert_eq!(path, vec![0, 1, 2]);
    }

    #[test]
    fn unreachable_returns_none() {
        let g = NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            vec![vec![], vec![]], // no edges
        );
        assert!(g.path(0, 1).is_none());
    }

    /// Plan 39: a water channel between two dry ledges. `generate` must sample water nodes,
    /// connect them with swim edges, bridge the ledges through the water (entry/exit), and
    /// A* must find a path from the left ledge to the right ledge that crosses water.
    #[test]
    fn water_channel_is_swimmable() {
        let cm = crate::collision::water_channel_world();
        // Span both side ledges (|x|>64) and the central water channel.
        let bounds = ([-144.0, -32.0, -16.0], [144.0, 32.0, 200.0]);
        let g = NavGraph::generate(&cm, bounds, 24.0);

        // Water nodes were sampled in the channel.
        let water_count = (0..g.node_count()).filter(|&i| g.is_water_node(i)).count();
        assert!(water_count > 0, "expected swim nodes in the channel");

        // At least one swim edge exists, and some swim edge bypasses STEP vertically
        // (a 3-D link the walk graph could never make).
        let (swim, _) = g.raw_swim_and_water();
        assert!(!swim.is_empty(), "expected swim edges");
        let steep_swim = swim
            .iter()
            .any(|&(a, b)| (g.nodes[a][2] - g.nodes[b][2]).abs() > STEP);
        assert!(
            steep_swim,
            "expected a swim edge steeper than STEP (3-D link)"
        );

        // Find a dry ledge node on each side (z≈24, |x|>64).
        let dry_left = (0..g.node_count())
            .find(|&i| !g.is_water_node(i) && g.nodes[i][0] < -64.0)
            .expect("left ledge node");
        let dry_right = (0..g.node_count())
            .find(|&i| !g.is_water_node(i) && g.nodes[i][0] > 64.0)
            .expect("right ledge node");

        // A* crosses the water from ledge to ledge.
        let path = g
            .path(dry_left, dry_right)
            .expect("path across the channel");
        assert!(
            path.iter().any(|&n| g.is_water_node(n)),
            "the path must go through water nodes"
        );
    }

    #[test]
    fn nearest_picks_closest() {
        let g = NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            vec![vec![], vec![]],
        );
        assert_eq!(g.nearest(&[95.0, 0.0, 0.0]), Some(1));
        assert_eq!(g.nearest(&[5.0, 0.0, 0.0]), Some(0));
    }

    /// Two equal-ish routes A→C: direct via B, longer via D. The unweighted path
    /// picks the shorter (A-B-C).
    fn diamond_graph() -> NavGraph {
        // 0=A(0,0,0) 1=B(0,100,0) 2=C(0,200,0) 3=D(100,100,0)
        NavGraph::from_raw(
            vec![
                [0.0, 0.0, 0.0],
                [0.0, 100.0, 0.0],
                [0.0, 200.0, 0.0],
                [100.0, 100.0, 0.0],
            ],
            vec![
                vec![(1, 100.0), (3, 141.0)], // A → B, D
                vec![(0, 100.0), (2, 100.0)], // B → A, C
                vec![(1, 100.0), (3, 141.0)], // C → B, D
                vec![(0, 141.0), (2, 141.0)], // D → A, C
            ],
        )
    }

    #[test]
    fn unweighted_picks_shorter_route() {
        let g = diamond_graph();
        let path = g.path(0, 2).unwrap();
        assert_eq!(path, vec![0, 1, 2], "unweighted A→C goes A-B-C (200 < 282)");
    }

    #[test]
    fn weighted_detours_around_dangerous_node() {
        let g = diamond_graph();
        // Make B (node 1) deadly → the A-B-C route is penalized, A-D-C wins.
        let overlay = vec![0.0, 1000.0, 0.0, 0.0];
        let path = g.path_weighted(0, 2, &overlay).unwrap();
        assert_eq!(path, vec![0, 3, 2], "danger at B routes via D");
    }

    #[test]
    fn weighted_gravitates_to_popular_node() {
        let g = diamond_graph();
        // Make B (node 1) very popular (negative overlay → cheap to leave) so
        // A-B-C is preferred even though A-D-C was equal/shorter in base length.
        let overlay = vec![0.0, -1000.0, 0.0, 0.0];
        let path = g.path_weighted(0, 2, &overlay).unwrap();
        assert_eq!(
            path,
            vec![0, 1, 2],
            "popularity at B pulls the route through B"
        );
    }

    #[test]
    fn weighted_unreachable_stays_none() {
        let g = NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            vec![vec![], vec![]],
        );
        assert!(g.path_weighted(0, 1, &[0.0, 0.0]).is_none());
    }

    // ── smooth_path tests (Plan 14 T1) ──────────────────────────────────────

    /// Straight-line path A→B→C in open air: string-pull collapses to [A, C].
    /// Nodes are 50u apart so the 120u MAX_SMOOTH_HDIST cap can see C from A (100u).
    #[test]
    fn smooth_path_straight_run_collapses() {
        let g = NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [50.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            vec![vec![(1, 50.0)], vec![(0, 50.0), (2, 50.0)], vec![(1, 50.0)]],
        );
        let cm = CollisionModel::half_space([1.0, 0.0, 0.0], -10000.0); // all-clear
        let path = vec![0, 1, 2];
        let smoothed = g.smooth_path(&cm, &path, [0.0, 0.0, 0.0]);
        // A→C (100u < 120u cap) is LOS-clear, so the middle node (1) is skipped.
        assert!(
            smoothed.len() < path.len(),
            "straight run: smoothed path ({}) should be shorter than raw ({})",
            smoothed.len(),
            path.len()
        );
        assert_eq!(*smoothed.last().unwrap(), 2, "goal node must be preserved");
    }

    /// L-shaped path A→B→C→D in open space (all-clear model): the string-pull
    /// should collapse all intermediate nodes since A has direct LOS to D.
    /// Nodes are 50u apart so A→D diagonal (112u) fits within MAX_SMOOTH_HDIST=120u.
    #[test]
    fn smooth_path_l_shape_open_collapses() {
        let g = NavGraph::from_raw(
            vec![
                [0.0, 0.0, 0.0],    // 0: A
                [50.0, 0.0, 0.0],   // 1: B
                [50.0, 50.0, 0.0],  // 2: C
                [50.0, 100.0, 0.0], // 3: D  — A→D = sqrt(50²+100²) ≈ 112u < 120u cap
            ],
            vec![
                vec![(1, 50.0)],
                vec![(0, 50.0), (2, 50.0)],
                vec![(1, 50.0), (3, 50.0)],
                vec![(2, 50.0)],
            ],
        );
        // All-clear model: wall at x<-10000 (never in range).
        let cm = CollisionModel::half_space([1.0, 0.0, 0.0], -10000.0);
        let path = vec![0, 1, 2, 3];
        let smoothed = g.smooth_path(&cm, &path, [0.0, 0.0, 0.0]);
        // All nodes within 120u of A → collapsed to [A, D].
        assert_eq!(smoothed[0], 0, "first node is A");
        assert_eq!(*smoothed.last().unwrap(), 3, "goal D is preserved");
        assert!(
            smoothed.len() <= path.len(),
            "smoothed is not longer than original"
        );
        assert_eq!(smoothed, vec![0, 3], "open path collapses to [A, D]");
    }

    /// A spawn off the grid is seeded and connected to the existing graph.
    #[test]
    fn seed_spawns_adds_and_connects() {
        // One existing node at origin; all-clear collision model.
        let cm = CollisionModel::half_space([1.0, 0.0, 0.0], -10000.0);
        let mut g = NavGraph::from_raw(vec![[0.0, 0.0, 0.0]], vec![vec![]]);
        // Spawn 80 u away (within 128u connect radius; no existing node within STEP=18).
        let n_before = g.node_count();
        let added = g.seed_spawns(&cm, &[[80.0, 0.0, 0.0]]);
        assert_eq!(added, 1, "one node seeded");
        assert_eq!(g.node_count(), n_before + 1, "graph grew");
        // New node should be connected to node 0.
        assert!(
            g.adj[1].iter().any(|&(nb, _)| nb == 0),
            "seeded node connected to existing graph"
        );
    }

    /// A spawn already close to an existing node is not double-seeded.
    #[test]
    fn seed_spawns_skips_nearby_node() {
        let cm = CollisionModel::half_space([1.0, 0.0, 0.0], -10000.0);
        let mut g = NavGraph::from_raw(vec![[0.0, 0.0, 0.0]], vec![vec![]]);
        // Spawn only 5 u away — within STEP=18, should be skipped.
        let added = g.seed_spawns(&cm, &[[5.0, 0.0, 0.0]]);
        assert_eq!(added, 0, "nearby spawn not duplicated");
    }

    /// spawns_in_largest_component counts correctly.
    #[test]
    fn spawns_connectivity_counts_correct() {
        // Two disconnected nodes; spawn nearest to node 0 (in largest component by tie).
        let g = NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]],
            vec![vec![], vec![]],
        );
        let (connected, total) = g.spawns_in_largest_component(&[[1.0, 0.0, 0.0]]);
        assert_eq!(total, 1);
        assert_eq!(connected, 1, "spawn maps to some component");
    }

    /// The PARALLEL prune classification must produce byte-for-byte the same result as a
    /// sequential pass — same edges, same order, same Trustworthy/Candidate split — so the
    /// parallelised prune is guaranteed deterministic and identical to the original.
    #[test]
    fn prune_classify_par_matches_seq() {
        // Solid below z=50, empty above (floor surface at z=50).
        let cm = CollisionModel::half_space([0.0, 0.0, 1.0], 50.0);
        // ~120 nodes, mostly at z=100 (clear) with every 7th at z=24 (below the floor → its
        // incoming edges trace into solid → Candidate). Enough nodes that rayon splits work.
        let mut nodes = Vec::new();
        for i in 0..120 {
            let z = if i % 7 == 0 { 24.0 } else { 100.0 };
            nodes.push([(i as f32) * 30.0, 0.0, z]);
        }
        let mut adj: Vec<Vec<(usize, f32)>> = vec![Vec::new(); nodes.len()];
        for i in 0..nodes.len() {
            for j in (i + 1)..(i + 5).min(nodes.len()) {
                let d = ((nodes[j][0] - nodes[i][0]).powi(2) + (nodes[j][2] - nodes[i][2]).powi(2))
                    .sqrt();
                adj[i].push((j, d));
                adj[j].push((i, d));
            }
        }
        let g = NavGraph::from_raw(nodes, adj);
        let max_hd = 100.0;
        let par = g.classify_prune_edges_par(&cm, max_hd);
        let seq = g.classify_prune_edges_seq(&cm, max_hd);
        assert_eq!(
            par, seq,
            "parallel classification must equal sequential, in order"
        );
        assert!(
            par.iter().any(|c| matches!(c, EdgeClass::Trustworthy(..))),
            "expected some Trustworthy edges"
        );
        assert!(
            par.iter().any(|c| matches!(c, EdgeClass::Candidate(..))),
            "expected some Candidate edges (blocked, non-stair)"
        );
    }

    /// detect_jump_edges creates a ledge-drop edge where a height-gapped pair
    /// of nodes has no walk edge (height diff > STEP).
    #[test]
    fn detect_jump_edges_adds_ledge_drop() {
        // half_space([0,0,1], 0): t = p.z - 0 = p.z; front (z>0) = EMPTY, back (z<0) = SOLID.
        // This gives a floor surface at z=0 for all x,y.
        let cm = CollisionModel::half_space([0.0, 0.0, 1.0], 0.0);
        // Node A is on a ledge at z=100; node B is on the floor at z=24.
        // Height diff = 76 > STEP=18 → no walk edge was added.
        let mut g = NavGraph::from_raw(
            vec![
                [0.0, 0.0, 100.0], // 0: A — ledge
                [96.0, 0.0, 24.0], // 1: B — floor below (96u away, 76u drop)
            ],
            vec![vec![], vec![]],
        );
        let added = g.detect_jump_edges(&cm, 64.0);
        assert!(added >= 1, "expected ≥1 jump edge, got {added}");
        // The A→B edge should be tagged as Jump.
        assert!(
            matches!(g.edge_kind(0, 1), EdgeKind::Jump { .. }),
            "A→B must be a Jump edge"
        );
        // No symmetric walk edge should exist (only a one-way jump down).
        // Walk edge A→B: if present, would be walk. Jump edge is in jump_edges set.
        // We just verify edge_kind for the pair we added.
    }

    /// Plan 52: the floor probe samples a V-groove duct via the hull-rest fallback —
    /// the point floor at the groove seam is too deep for the 32×32 hull (stationary
    /// check startsolid), but the hull rests on the slopes 16u higher, like pmove.
    #[test]
    fn floor_probe_samples_v_groove_via_hull_rest() {
        let cm = crate::collision::v_groove_world();
        let bounds = ([-100.0, -100.0, -50.0], [100.0, 100.0, 200.0]);
        // Groove seam column (y=0): point floor z=0, hull rests at origin z=40.
        let seam = floor_waypoints_multi(&cm, 0.0, 0.0, bounds);
        assert_eq!(seam.len(), 1, "seam column must yield exactly one node");
        assert!(
            (seam[0][2] - 40.0).abs() < 1.0,
            "hull rests at origin z≈40, got {}",
            seam[0][2]
        );
        // Off-seam column (y=5): point floor z=5, rest origin z=45.
        let off = floor_waypoints_multi(&cm, 0.0, 5.0, bounds);
        assert_eq!(off.len(), 1);
        assert!(
            (off[0][2] - 45.0).abs() < 1.0,
            "hull rests at origin z≈45, got {}",
            off[0][2]
        );
    }

    /// Plan 52: a spawn-bearing ledge whose only exit is a drop deeper than the
    /// universal jump-bridge cap (256) but within the rescue cap (384) gets one
    /// jump-down edge to the play component, flipping the connectivity gate.
    #[test]
    fn rescue_bridges_stranded_spawn_ledge() {
        // Floor surface at z=0 everywhere; ledge node A at z=324 (300u above the
        // floor nodes at z=24 — past 256, within 384), 96u horizontal.
        let cm = CollisionModel::half_space([0.0, 0.0, 1.0], 0.0);
        let mut g = NavGraph::from_raw(
            vec![
                [0.0, 0.0, 324.0],  // 0: A — stranded ledge, has a spawn
                [96.0, 0.0, 24.0],  // 1: B — floor (play component)
                [144.0, 0.0, 24.0], // 2: C — floor, connected to B
            ],
            vec![vec![], vec![(2, 48.0)], vec![(1, 48.0)]],
        );
        let spawns = [[0.0, 0.0, 324.0], [96.0, 0.0, 24.0], [144.0, 0.0, 24.0]];
        // Sanity: the normal cap can't bridge a 300u drop.
        assert_eq!(g.bridge_components_via_jump(&cm, 104.0, 256.0, 6), 0);
        assert_eq!(g.spawns_in_largest_component(&spawns), (2, 3));

        let added = g.rescue_stranded_spawns(&cm, 104.0, 384.0, &spawns);
        assert_eq!(added, 1, "one rescue edge for the one stranded component");
        assert!(
            matches!(g.edge_kind(0, 1), EdgeKind::Jump { .. }),
            "A→B must be a Jump edge"
        );
        assert_eq!(
            g.spawns_in_largest_component(&spawns),
            (3, 3),
            "gate flips to all-connected"
        );
    }

    /// Plan 52: the rescue pass never links spawnless components — the same geometry
    /// with no spawn on the ledge stays split.
    #[test]
    fn rescue_ignores_spawnless_components() {
        let cm = CollisionModel::half_space([0.0, 0.0, 1.0], 0.0);
        let mut g = NavGraph::from_raw(
            vec![
                [0.0, 0.0, 324.0],  // 0: spawnless ledge
                [96.0, 0.0, 24.0],  // 1: floor
                [144.0, 0.0, 24.0], // 2: floor
            ],
            vec![vec![], vec![(2, 48.0)], vec![(1, 48.0)]],
        );
        let spawns = [[96.0, 0.0, 24.0], [144.0, 0.0, 24.0]];
        let added = g.rescue_stranded_spawns(&cm, 104.0, 384.0, &spawns);
        assert_eq!(added, 0, "spawnless ledge must not be rescued");
        assert_eq!(g.components().len(), 2, "components stay split");
    }

    /// Path of length ≤2 is returned unchanged.
    #[test]
    fn smooth_path_short_path_unchanged() {
        let g = NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            vec![vec![(1, 100.0)], vec![(0, 100.0)]],
        );
        let cm = CollisionModel::half_space([1.0, 0.0, 0.0], -10000.0);
        let path = vec![0, 1];
        let smoothed = g.smooth_path(&cm, &path, [0.0, 0.0, 0.0]);
        assert_eq!(smoothed, path, "length-2 path is returned as-is");
    }
}
