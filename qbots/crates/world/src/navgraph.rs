//! Nav graph — auto-generated waypoints + A* pathfinding.
//!
//! The genuinely original part of the world model (no bot archive did this externally):
//! sample walkable floor positions on a grid, connect neighbors whose edge clears a
//! player-box trace, then A* over the graph.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};

use rayon::prelude::*;

use crate::collision::{CollisionModel, MASK_SOLID, MASK_WATER};

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

/// Number of grid cells to scan per axis to fully COVER [`CONNECT_RADIUS`] at a given
/// `spacing`. Uses `ceil` (not `round`) so the scan always reaches ≥ the radius; the
/// generate() loop then filters candidates to the exact ±`CONNECT_RADIUS` per-axis window.
/// This decouples the connection radius from the integer cell count — critical for grid
/// spacings that don't divide the radius evenly (e.g. round(72/16)=5→80u was wrong; ceil
/// →5 cells scanned, then trimmed to exactly 72u). `≥1` always.
pub fn connect_cells(spacing: f32) -> i32 {
    ((CONNECT_RADIUS / spacing).ceil() as i32).max(1)
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
}

/// A navigation graph: waypoints (bot-origin positions) + LOS-checked edges.
pub struct NavGraph {
    pub nodes: Vec<[f32; 3]>,
    adj: Vec<Vec<(usize, f32)>>,         // (neighbor index, edge cost)
    jump_edges: HashSet<(usize, usize)>, // (from, to) pairs that are jump edges
    jump_yaws: HashMap<(usize, usize), f32>, // launch_yaw per jump edge
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
        let mut hits: Vec<((i32, i32), [f32; 3])> = columns
            .par_iter()
            .flat_map_iter(|&(x, y, gx, gy)| {
                floor_waypoints_multi(cm, x, y, bounds)
                    .into_iter()
                    .map(move |wp| ((gx, gy), wp))
            })
            .collect();

        // Sort by (grid_key, z) so node indices are deterministic across runs.
        hits.sort_by(|((ax, ay), aw), ((bx, by), bw)| {
            (*ax, *ay)
                .cmp(&(*bx, *by))
                .then_with(|| aw[2].total_cmp(&bw[2]))
        });

        // Grid maps each column to all node indices it contains (ordered by z asc).
        let mut nodes: Vec<[f32; 3]> = Vec::with_capacity(hits.len());
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::with_capacity(hits.len());
        for ((gx, gy), wp) in hits {
            let idx = nodes.len();
            grid.entry((gx, gy)).or_default().push(idx);
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
        let adj: Vec<Vec<(usize, f32)>> = nodes
            .par_iter()
            .enumerate()
            .map(|(i, &a)| {
                let gx = (a[0] / spacing).round() as i32;
                let gy = (a[1] / spacing).round() as i32;
                let mut edges = Vec::new();
                // Connect a neighbourhood of ±cells grid cells, not just the 8 immediate
                // neighbours. A ±1 (24u) connection misses real walkable links that span
                // 2-4 cells — e.g. across a ramp/step where the intermediate column has no
                // sampled node — which fragments the graph into dozens of false components
                // (proven by tools/compgaps: 934 missed walkable links on q2dm1). The
                // per-pair hull/stair check below still rejects wall-separated pairs, so
                // widening only adds genuinely walkable edges.
                for ddx in -cells..=cells {
                    for ddy in -cells..=cells {
                        if ddx == 0 && ddy == 0 {
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
                            let dz = b[2] - a[2];
                            if dz.abs() > STAIR_MAX {
                                continue; // too steep for stairs — cliff or void
                            }
                            let ok = if dz.abs() <= STEP {
                                // Flat or gentle slope: try direct hull trace first.
                                // Fall back to the step-climb trace when the direct trace
                                // fails — stair risers can clip the diagonal even for
                                // small height deltas.
                                let t = cm.trace(&a, &b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                                if !t.startsolid && t.fraction >= 1.0 {
                                    true
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
                                edges.push((j, dist(&a, &b)));
                            }
                        }
                    }
                }
                edges
            })
            .collect();

        NavGraph {
            nodes,
            adj,
            jump_edges: HashSet::new(),
            jump_yaws: HashMap::new(),
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn edge_count(&self) -> usize {
        self.adj.iter().map(|e| e.len()).sum()
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
        let max_hd2 = max_hd * max_hd;
        let mut uf = UnionFind::new(n);
        // Candidate undirected edges (false: long-blocked or flat-blocked), span for sort.
        let mut candidates: Vec<(usize, usize, f32)> = Vec::new();
        for a in 0..n {
            let pa = self.nodes[a];
            for &(b, _) in &self.adj[a] {
                if a >= b {
                    continue; // visit each undirected edge once
                }
                let pb = self.nodes[b];
                let hd2 = (pb[0] - pa[0]).powi(2) + (pb[1] - pa[1]).powi(2);
                let dz = (pb[2] - pa[2]).abs();
                let t = cm.trace(&pa, &pb, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                let blocked = t.startsolid || t.fraction < 0.999;
                if !blocked {
                    uf.union(a, b); // hull-clear: trustworthy, part of the base graph
                    continue;
                }
                // Blocked straight line. A short steep edge MIGHT be a real staircase the
                // bot climbs via pmove stepping — confirm with walkable_stair. If that also
                // fails (stair=NO), it is a false cliff edge (e.g. the q2dm1 weapon's
                // 3972→3978 dz=128 hd=96) that traps bots at a fake shortcut → candidate.
                if hd2 <= max_hd2 && dz > STEP {
                    let (lo, hi) = if pa[2] < pb[2] { (pa, pb) } else { (pb, pa) };
                    if walkable_stair(cm, lo, hi) {
                        uf.union(a, b); // real stair
                        continue;
                    }
                }
                // Flat-blocked, long-blocked, or steep-but-not-a-stair → false edge.
                candidates.push((a, b, hd2));
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
        }
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
        // Using `j > i` enumerates each unordered pair exactly once, halving trace
        // calls vs. the symmetric loop. Collect candidates first (immutable borrow),
        // then add edges (mutable borrow).
        let max_h2 = max_hdist * max_hdist;
        let mut candidates: Vec<(usize, usize, f32)> = Vec::new();
        for i in 0..n {
            let a = self.nodes[i];
            let (kx, ky) = key(&a);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(cellnodes) = buckets.get(&(kx + dx, ky + dy)) else {
                        continue;
                    };
                    for &j in cellnodes {
                        if j <= i {
                            continue; // enumerate each pair once
                        }
                        if comp_id[j] == comp_id[i] {
                            continue; // same component — already linked
                        }
                        let b = self.nodes[j];
                        if (b[2] - a[2]).abs() > STAIR_MAX {
                            continue;
                        }
                        let h2 = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
                        if h2 > max_h2 {
                            continue;
                        }
                        if walkable_stair_link_orig(cm, a, b) {
                            let dz = (b[2] - a[2]).abs();
                            let hdist = h2.sqrt();
                            let slope = if hdist > 0.1 { dz / hdist } else { 999.0 };
                            if dz > 100.0 {
                                tracing::debug!(
                                    slope = slope as u32,
                                    dz = dz as u32,
                                    hdist = hdist as u32,
                                    ax = a[0] as i32,
                                    ay = a[1] as i32,
                                    az = a[2] as i32,
                                    bx = b[0] as i32,
                                    by = b[1] as i32,
                                    bz = b[2] as i32,
                                    "bridge: large-dz edge added"
                                );
                            }
                            candidates.push((i, j, dist(&a, &b)));
                        }
                    }
                }
            }
        }

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
        let mut added = 0;
        let n = self.nodes.len();

        for a in 0..n {
            let an = self.nodes[a];
            for (dx, dy) in DIRS {
                let px = an[0] + dx * probe_dist;
                let py = an[1] + dy * probe_dist;

                // Down-trace from above probe to below A - MAX_FALL.
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
                let landing = [px, py, floor_z + 24.0];

                // Find the nearest sampled node B at the landing site.
                let Some(b) = self.nearest(&landing) else {
                    continue;
                };
                let bn = self.nodes[b];
                if b == a {
                    continue;
                }
                // B must be close to landing horizontally and within height band.
                let dx2 = (bn[0] - landing[0]).powi(2) + (bn[1] - landing[1]).powi(2);
                if dx2 > (STEP * 2.0).powi(2) {
                    continue;
                }

                // Skip if a walk edge already exists.
                if self.adj[a].iter().any(|&(nb, _)| nb == b) {
                    continue;
                }
                // Skip if jump edge already recorded.
                if self.jump_edges.contains(&(a, b)) {
                    continue;
                }

                // Ensure the first half of the path from A to B is clear (no wall).
                let t = cm.trace(&an, &bn, &zero, &zero, MASK_SOLID);
                if t.startsolid || t.fraction < 0.4 {
                    continue;
                }

                let launch_yaw = dy.atan2(dx).to_degrees();
                let cost = dist(&an, &bn);
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

        if cm.point_contents(&wp) & MASK_WATER == 0 {
            let stand = cm.trace(&wp, &wp, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
            if !stand.startsolid {
                results.push(wp);
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
        let down = [p[0], p[1], p[2] - FLOOR_PROBE];
        let t = cm.trace(&p, &down, &zero, &zero, MASK_SOLID);
        // No floor within FLOOR_PROBE (fraction == 1.0) → gap under the shortcut.
        if t.fraction >= 1.0 && !t.startsolid {
            return false;
        }
    }
    true
}

/// block the path. Stair risers don't block the upward vertical traces, and the
/// horizontal traces at each stepped height clear any risers below that level.
///
/// `upper[2] > lower[2]` is assumed; `total_dz` must be in `(0, STAIR_MAX]`.
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
        let fwd = cm.trace(&a, &b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if !fwd.startsolid && fwd.fraction >= 1.0 {
            return true;
        }
        let rev = cm.trace(&b, &a, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if !rev.startsolid && rev.fraction >= 1.0 {
            return true;
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
