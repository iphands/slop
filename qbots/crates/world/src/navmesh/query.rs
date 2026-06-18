//! Phase 3 — polygon A* + funnel → a smooth 3D polyline.
//!
//! `NavMesh::path` runs A* over the portal graph (poly-center distance cost, straight-line
//! heuristic) to get a corridor of polygons, then the **Simple Stupid Funnel Algorithm**
//! pulls a taut path through the portal edges. Portals are **inset by the agent radius**, so
//! the line threads doorways down their centerline and never scrapes a wall — the property
//! that makes navmesh movement density-independent and wall-clear. Heights come from the
//! corridor polys (the funnel itself is 2D).
//!
//! Funnel reference: Mikko Mononen, "Simple Stupid Funnel Algorithm".

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::navmesh::polymesh::NavMesh;

/// A* open-set entry, ordered as a min-heap on `f` (total cost estimate).
struct State {
    f: f32,
    poly: u32,
}
impl PartialEq for State {
    fn eq(&self, o: &Self) -> bool {
        self.f == o.f
    }
}
impl Eq for State {}
impl Ord for State {
    fn cmp(&self, o: &Self) -> Ordering {
        // Reverse so BinaryHeap (a max-heap) pops the smallest f first.
        o.f.total_cmp(&self.f)
    }
}
impl PartialOrd for State {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl NavMesh {
    /// Plan a smooth path from `start` to `goal` (world positions). Returns a polyline
    /// (`start … goal`) the bot can pure-pursue, or `None` if either end is off-mesh or no
    /// polygon corridor connects them. `agent_radius` insets portals so the path clears walls.
    pub fn path(
        &self,
        start: [f32; 3],
        goal: [f32; 3],
        agent_radius: f32,
    ) -> Option<Vec<[f32; 3]>> {
        let s = self.nearest_poly(start)?;
        let g = self.nearest_poly(goal)?;
        if s == g {
            return Some(vec![start, goal]);
        }
        let corridor = self.astar(s, g)?;
        Some(self.funnel(start, goal, &corridor, agent_radius))
    }

    /// A* over the portal graph from poly `s` to poly `g`. Returns the polygon corridor.
    fn astar(&self, s: usize, g: usize) -> Option<Vec<usize>> {
        let n = self.polys.len();
        let mut g_score = vec![f32::INFINITY; n];
        let mut came: Vec<u32> = vec![u32::MAX; n];
        let goal_c = self.poly_center(g);
        g_score[s] = 0.0;
        let mut open = BinaryHeap::new();
        open.push(State {
            f: dist(self.poly_center(s), goal_c),
            poly: s as u32,
        });

        while let Some(State { poly, .. }) = open.pop() {
            let p = poly as usize;
            if p == g {
                // Reconstruct.
                let mut path = vec![g];
                let mut cur = g;
                while cur != s {
                    cur = came[cur] as usize;
                    path.push(cur);
                }
                path.reverse();
                return Some(path);
            }
            let pc = self.poly_center(p);
            let base = g_score[p];
            for &q in &self.adj[p] {
                let qi = q as usize;
                let tentative = base + dist(pc, self.poly_center(qi));
                if tentative < g_score[qi] {
                    g_score[qi] = tentative;
                    came[qi] = p as u32;
                    open.push(State {
                        f: tentative + dist(self.poly_center(qi), goal_c),
                        poly: q,
                    });
                }
            }
        }
        None
    }

    /// Build the inset left/right portal sequence for `corridor` and string-pull it.
    fn funnel(
        &self,
        start: [f32; 3],
        goal: [f32; 3],
        corridor: &[usize],
        agent_radius: f32,
    ) -> Vec<[f32; 3]> {
        // Portal 0 = start (degenerate), then one per poly boundary, then goal (degenerate).
        let mut left: Vec<[f32; 2]> = Vec::with_capacity(corridor.len() + 1);
        let mut right: Vec<[f32; 2]> = Vec::with_capacity(corridor.len() + 1);
        left.push([start[0], start[1]]);
        right.push([start[0], start[1]]);

        for w in corridor.windows(2) {
            let (a, b) = (w[0], w[1]);
            if !self.grid_adjacent(a, b) {
                // A bridged stair/ramp link: no shared grid edge. Pinch the funnel through the
                // midpoint of the two poly centers (a forced waypoint) so the path takes the
                // bridge rather than cutting across the gap it spans.
                let ca = self.poly_center(a);
                let cb = self.poly_center(b);
                let mid = [(ca[0] + cb[0]) * 0.5, (ca[1] + cb[1]) * 0.5];
                left.push(mid);
                right.push(mid);
                continue;
            }
            let [e0, e1] = self.portal(a, b);
            let ca = self.poly_center(a);
            let dir = [
                self.poly_center(b)[0] - ca[0],
                self.poly_center(b)[1] - ca[1],
            ];
            // The "left" endpoint is the one CCW of the travel direction.
            let s0 = cross(dir, [e0[0] - ca[0], e0[1] - ca[1]]);
            let s1 = cross(dir, [e1[0] - ca[0], e1[1] - ca[1]]);
            let (mut l, mut r) = if s0 >= s1 {
                ([e0[0], e0[1]], [e1[0], e1[1]])
            } else {
                ([e1[0], e1[1]], [e0[0], e0[1]])
            };
            inset_portal(&mut l, &mut r, agent_radius);
            left.push(l);
            right.push(r);
        }
        left.push([goal[0], goal[1]]);
        right.push([goal[0], goal[1]]);

        let pts2d = string_pull(&left, &right);

        // Lift each 2D point to 3D using the corridor poly whose center is 2D-nearest (avoids
        // stacked-floor ambiguity that a global nearest_poly would hit).
        pts2d
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if i == 0 {
                    start
                } else if i == pts2d.len() - 1 {
                    goal
                } else {
                    [p[0], p[1], self.corridor_height(*p, corridor)]
                }
            })
            .collect()
    }

    /// Height for a 2D funnel point: the origin-Z of the corridor poly whose center is
    /// horizontally nearest.
    fn corridor_height(&self, p: [f32; 2], corridor: &[usize]) -> f32 {
        let mut best = (f32::INFINITY, 0.0);
        for &poly in corridor {
            let c = self.poly_center(poly);
            let d = (c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2);
            if d < best.0 {
                best = (d, c[2]);
            }
        }
        best.1
    }
}

/// Move portal endpoints toward each other by `radius` (clamped to the midpoint) so the
/// funnel path clears walls / threads narrow doorways centrally.
fn inset_portal(l: &mut [f32; 2], r: &mut [f32; 2], radius: f32) {
    let dx = r[0] - l[0];
    let dy = r[1] - l[1];
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-3 {
        return;
    }
    let inset = radius.min(len * 0.5 - 0.1).max(0.0);
    let ux = dx / len;
    let uy = dy / len;
    l[0] += ux * inset;
    l[1] += uy * inset;
    r[0] -= ux * inset;
    r[1] -= uy * inset;
}

/// Simple Stupid Funnel Algorithm over (left, right) portal endpoints. Returns the taut
/// 2D corner sequence from `left[0]` (= start) to `left.last()` (= goal).
fn string_pull(left: &[[f32; 2]], right: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut pts = Vec::new();
    let mut apex = left[0];
    let mut p_left = left[0];
    let mut p_right = right[0];
    let (mut left_i, mut right_i) = (0usize, 0usize);
    pts.push(apex);

    let mut i = 1;
    while i < left.len() {
        let l = left[i];
        let r = right[i];

        // Tighten the right side.
        if tri_area2(apex, p_right, r) <= 0.0 {
            if eq(apex, p_right) || tri_area2(apex, p_left, r) > 0.0 {
                p_right = r;
                right_i = i;
            } else {
                // Right crossed left → the left endpoint becomes the new apex; restart from it.
                pts.push(p_left);
                apex = p_left;
                p_right = apex;
                right_i = left_i;
                i = left_i + 1;
                continue;
            }
        }
        // Tighten the left side.
        if tri_area2(apex, p_left, l) >= 0.0 {
            if eq(apex, p_left) || tri_area2(apex, p_right, l) < 0.0 {
                p_left = l;
                left_i = i;
            } else {
                // Left crossed right → the right endpoint becomes the new apex; restart from it.
                pts.push(p_right);
                apex = p_right;
                p_left = apex;
                left_i = right_i;
                i = right_i + 1;
                continue;
            }
        }
        i += 1;
    }
    let goal = left[left.len() - 1];
    if !eq(*pts.last().unwrap(), goal) {
        pts.push(goal);
    }
    pts
}

#[inline]
fn cross(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}

/// Signed area term using Mononen's SSF winding convention (`bx*ay - ax*by`), the sign the
/// `string_pull` comparisons below are written against. (This is the negation of the usual
/// cross-product orientation, which is why the funnel comparisons look "backwards".)
#[inline]
fn tri_area2(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (c[0] - a[0]) * (b[1] - a[1]) - (b[0] - a[0]) * (c[1] - a[1])
}

#[inline]
fn eq(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
}

#[inline]
fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navmesh::heightfield::Heightfield;

    /// Straight 4-cell corridor: the funnel returns essentially a straight line (start→goal),
    /// no zig-zag through cell centers.
    #[test]
    fn straight_corridor_funnels_to_a_line() {
        let hf = Heightfield {
            cell_size: 16.0,
            nx: 4,
            ny: 1,
            min: [0.0, 0.0],
            columns: vec![vec![24.0]; 4],
        };
        let mesh = NavMesh::build(&hf, 18.0);
        let path = mesh
            .path([8.0, 8.0, 24.0], [56.0, 8.0, 24.0], 0.0)
            .expect("path exists");
        // All points collinear in y (straight along x).
        for p in &path {
            assert!(
                (p[1] - 8.0).abs() < 1.0,
                "point off the straight line: {p:?}"
            );
        }
    }

    /// L-shaped corridor: the funnel cuts the inside corner — the path's middle point sits
    /// near the inside corner, not out at the cell centers.
    #[test]
    fn l_corridor_cuts_the_inside_corner() {
        // 3 cells along x at y=0, then 3 cells up in y at x=2 → an L.
        let nx = 3;
        let ny = 3;
        let mut columns = vec![Vec::new(); nx * ny];
        for ix in 0..nx {
            columns[ix] = vec![24.0]; // bottom row
        }
        for iy in 0..ny {
            columns[iy * nx + 2] = vec![24.0]; // right column
        }
        let hf = Heightfield {
            cell_size: 16.0,
            nx,
            ny,
            min: [0.0, 0.0],
            columns,
        };
        let mesh = NavMesh::build(&hf, 18.0);
        // From bottom-left cell to top-right cell.
        let path = mesh
            .path([8.0, 8.0, 24.0], [40.0, 40.0, 24.0], 0.0)
            .expect("path exists");
        assert!(path.len() >= 2);
        // The taut path turns near the inside corner (around x=32, y=16), not via (8,40).
        let turned_near_corner = path
            .iter()
            .any(|p| (p[0] - 32.0).abs() < 12.0 && (p[1] - 16.0).abs() < 12.0);
        assert!(turned_near_corner, "funnel didn't cut the corner: {path:?}");
    }
}
