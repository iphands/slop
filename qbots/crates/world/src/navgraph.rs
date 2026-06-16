//! Nav graph — auto-generated waypoints + A* pathfinding.
//!
//! The genuinely original part of the world model (no bot archive did this externally):
//! sample walkable floor positions on a grid, connect neighbors whose edge clears a
//! player-box trace, then A* over the graph.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::collision::{CollisionModel, MASK_SOLID, MASK_WATER};

/// Q2 standing player hull (`VEC_HULL_MIN/MAX`): the bbox traces use.
pub const HULL_MINS: [f32; 3] = [-16.0, -16.0, -24.0];
pub const HULL_MAXS: [f32; 3] = [16.0, 16.0, 32.0];
/// Max walkable height delta between adjacent waypoints (a "step").
const STEP: f32 = 24.0;
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
    /// whose edge is clear and step is small.
    pub fn generate(cm: &CollisionModel, bounds: ([f32; 3], [f32; 3]), spacing: f32) -> Self {
        let mut nodes: Vec<[f32; 3]> = Vec::new();
        let mut grid: HashMap<(i32, i32), usize> = HashMap::new();

        let mut x = bounds.0[0];
        while x <= bounds.1[0] {
            let mut y = bounds.0[1];
            while y <= bounds.1[1] {
                if let Some(wp) = floor_waypoint(cm, x, y, bounds) {
                    let gx = (x / spacing).round() as i32;
                    let gy = (y / spacing).round() as i32;
                    grid.insert((gx, gy), nodes.len());
                    nodes.push(wp);
                }
                y += spacing;
            }
            x += spacing;
        }

        let mut adj: Vec<Vec<(usize, f32)>> = vec![Vec::new(); nodes.len()];
        for (&(gx, gy), &i) in &grid {
            let a = nodes[i];
            for ddx in -1..=1i32 {
                for ddy in -1..=1i32 {
                    if ddx == 0 && ddy == 0 {
                        continue;
                    }
                    if let Some(&j) = grid.get(&(gx + ddx, gy + ddy)) {
                        let b = nodes[j];
                        if (a[2] - b[2]).abs() > STEP {
                            continue; // too big a step
                        }
                        let t = cm.trace(&a, &b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                        if t.fraction < 1.0 || t.startsolid {
                            continue; // wall in the way (or stuck)
                        }
                        let cost = dist(&a, &b);
                        adj[i].push((j, cost));
                    }
                }
            }
        }

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

    /// Connected components (BFS). Useful for diagnosing multi-level fragmentation.
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

        let mut result = Vec::new();
        result.push(path[0]);

        let mut apex = from;
        let mut commit_idx = 0usize; // index in `path` we most recently committed

        while commit_idx < path.len() - 1 {
            // Scan forward from apex for the furthest LOS-clear node.
            let mut furthest = commit_idx;
            let zero = [0.0f32; 3];
            for (j, &node_idx) in path.iter().enumerate().skip(commit_idx + 1) {
                let candidate = self.nodes[node_idx];
                let t = cm.trace(&apex, &candidate, &zero, &zero, MASK_SOLID);
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

        let added = to_add.len();
        for wp in to_add {
            let new_idx = self.nodes.len();
            self.nodes.push(wp);
            self.adj.push(Vec::new());
            // Connect to all existing nodes (pre-this-addition) within reach.
            for other_idx in 0..new_idx {
                let other = self.nodes[other_idx];
                if (wp[2] - other[2]).abs() >= STEP {
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
                let t = cm.trace(&wp, &other, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
                if t.fraction < 1.0 || t.startsolid {
                    continue;
                }
                let cost = dist(&wp, &other);
                self.adj[new_idx].push((other_idx, cost));
                self.adj[other_idx].push((new_idx, cost));
            }
        }
        added
    }

    /// Check how many of the given spawn positions are in the largest connected
    /// component. Returns `(in_largest, total)`. Used to log a warning when spawns
    /// are unreachable after `seed_spawns`.
    pub fn spawns_in_largest_component(&self, spawns: &[[f32; 3]]) -> (usize, usize) {
        if spawns.is_empty() {
            return (0, 0);
        }
        let largest: HashSet<usize> = self
            .components()
            .into_iter()
            .next()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let connected = spawns
            .iter()
            .filter(|sp| self.nearest(sp).is_some_and(|i| largest.contains(&i)))
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
                    let f = ng + dist(&self.nodes[nb], &self.nodes[goal]);
                    open.push(Reverse((FOrd(f), nb)));
                }
            }
        }
        None
    }
}

/// Find the walkable floor waypoint at column (x,y): trace down for the surface, then
/// confirm the player hull can stand there.
fn floor_waypoint(
    cm: &CollisionModel,
    x: f32,
    y: f32,
    bounds: ([f32; 3], [f32; 3]),
) -> Option<[f32; 3]> {
    let top = [x, y, bounds.1[2] + 200.0];
    let bot = [x, y, bounds.0[2] - 200.0];
    let down = cm.trace(&top, &bot, &[0.0; 3], &[0.0; 3], MASK_SOLID);
    if down.fraction >= 1.0 || down.startsolid {
        return None; // open shaft or started in solid
    }
    let floor_z = down.endpos[2];
    // bot origin stands ~24 above the floor (hull mins.z = -24).
    let wp = [x, y, floor_z + 24.0];
    // Skip waypoints inside water, slime or lava.
    if cm.point_contents(&wp) & MASK_WATER != 0 {
        return None;
    }
    let stand = cm.trace(&wp, &wp, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
    (!stand.startsolid).then_some(wp)
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
    #[test]
    fn smooth_path_straight_run_collapses() {
        let g = NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [200.0, 0.0, 0.0]],
            vec![
                vec![(1, 100.0)],
                vec![(0, 100.0), (2, 100.0)],
                vec![(1, 100.0)],
            ],
        );
        let cm = CollisionModel::half_space([1.0, 0.0, 0.0], -10000.0); // all-clear
        let path = vec![0, 1, 2];
        let smoothed = g.smooth_path(&cm, &path, [0.0, 0.0, 0.0]);
        // A→C is LOS-clear, so the middle node (1) should be skipped.
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
    /// This verifies maximum compression in obstacle-free environments.
    /// Note: testing "corner preservation when a wall blocks the shortcut" requires
    /// a finite-obstacle BSP model; a half_space's PLANE_X shortcut ignores normal
    /// sign, making negative-normal walls unreliable for this purpose.
    #[test]
    fn smooth_path_l_shape_open_collapses() {
        let g = NavGraph::from_raw(
            vec![
                [0.0, 0.0, 0.0],     // 0: A
                [100.0, 0.0, 0.0],   // 1: B
                [100.0, 100.0, 0.0], // 2: C
                [100.0, 200.0, 0.0], // 3: D
            ],
            vec![
                vec![(1, 100.0)],
                vec![(0, 100.0), (2, 100.0)],
                vec![(1, 100.0), (3, 100.0)],
                vec![(2, 100.0)],
            ],
        );
        // All-clear model: wall at x<-10000 (never in range).
        let cm = CollisionModel::half_space([1.0, 0.0, 0.0], -10000.0);
        let path = vec![0, 1, 2, 3];
        let smoothed = g.smooth_path(&cm, &path, [0.0, 0.0, 0.0]);
        // All nodes are in LOS from A, so A→D is clear → collapsed to [A, D].
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
        // Spawn 80 u away (within 128u connect radius; no existing node within STEP=24).
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
        // Spawn only 5 u away — within STEP=24, should be skipped.
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
        // Height diff = 76 > STEP=24 → no walk edge was added.
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
