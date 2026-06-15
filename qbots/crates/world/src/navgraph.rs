//! Nav graph — auto-generated waypoints + A* pathfinding.
//!
//! The genuinely original part of the world model (no bot archive did this externally):
//! sample walkable floor positions on a grid, connect neighbors whose edge clears a
//! player-box trace, then A* over the graph.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};

use crate::collision::{CollisionModel, MASK_SOLID};

/// Q2 standing player hull (`VEC_HULL_MIN/MAX`): the bbox traces use.
pub const HULL_MINS: [f32; 3] = [-16.0, -16.0, -24.0];
pub const HULL_MAXS: [f32; 3] = [16.0, 16.0, 32.0];
/// Max walkable height delta between adjacent waypoints (a "step").
const STEP: f32 = 24.0;

/// A navigation graph: waypoints (bot-origin positions) + LOS-checked edges.
pub struct NavGraph {
    pub nodes: Vec<[f32; 3]>,
    adj: Vec<Vec<(usize, f32)>>, // (neighbor index, edge cost)
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
                        if (a[2] - b[2]).abs() >= STEP {
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

        NavGraph { nodes, adj }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn edge_count(&self) -> usize {
        self.adj.iter().map(|e| e.len()).sum()
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
            for &(nb, cost) in &self.adj[cur] {
                if closed[nb] {
                    continue;
                }
                let ng = g[cur] + cost;
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
        let g = NavGraph {
            nodes: vec![[0.0, 0.0, 0.0], [64.0, 0.0, 0.0], [128.0, 0.0, 0.0]],
            adj: vec![vec![(1, 64.0)], vec![(0, 64.0), (2, 64.0)], vec![(1, 64.0)]],
        };
        let path = g.path(0, 2).unwrap();
        assert_eq!(path, vec![0, 1, 2]);
    }

    #[test]
    fn unreachable_returns_none() {
        let g = NavGraph {
            nodes: vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            adj: vec![vec![], vec![]], // no edges
        };
        assert!(g.path(0, 1).is_none());
    }

    #[test]
    fn nearest_picks_closest() {
        let g = NavGraph {
            nodes: vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            adj: vec![vec![], vec![]],
        };
        assert_eq!(g.nearest(&[95.0, 0.0, 0.0]), Some(1));
        assert_eq!(g.nearest(&[5.0, 0.0, 0.0]), Some(0));
    }
}
