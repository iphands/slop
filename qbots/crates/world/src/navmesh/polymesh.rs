//! Phase 2 (rev. 2) — walkable polygons via **greedy rectangle merge** + portal adjacency.
//!
//! Walkable cell-spans from the [`Heightfield`] are merged into maximal axis-aligned
//! **rectangles** that are z-coherent (all within `walkable_climb` of the seed, so a rect is
//! roughly planar — flat floors become big rects, stairs become thin per-step rects). Two
//! rectangles that share part of an edge (and are within `walkable_climb`) get a **portal** =
//! the *overlap* of their touching edges, which can be many cells wide.
//!
//! Why rectangles, not per-cell quads: the funnel insets portals by the agent radius to keep
//! the path off walls. A per-cell portal is only `cell_size` wide, so the inset collapses it
//! and the funnel zig-zags through cell centers. Wide rectangle portals inset cleanly, so the
//! funnel produces straight, wall-clear paths. Merging also cuts the poly count ~100×.
//!
//! Connectivity within `walkable_climb` (Q2 `STEP` = 18) links a staircase's per-step rects,
//! while stacked floors (z apart by ≫ STEP) stay separate components.

use rayon::prelude::*;

use crate::collision::CollisionModel;
use crate::navgraph::{walkable_stair, STAIR_MAX};
use crate::navmesh::heightfield::Heightfield;

/// One walkable rectangle: cells `[ix, ix+w) × [iy, iy+h)` at player-origin height `oz`.
#[derive(Debug, Clone, Copy)]
pub struct Poly {
    pub ix: u32,
    pub iy: u32,
    pub w: u32,
    pub h: u32,
    /// Player-origin Z (floor surface + 24).
    pub oz: f32,
}

/// A polygon navmesh: merged walkable rectangles + portal adjacency, plus the grid index
/// needed for `nearest_poly` and A*. Geometry is `[f32; 3]` (the crate stays glam-free).
pub struct NavMesh {
    pub cell_size: f32,
    pub nx: usize,
    pub ny: usize,
    /// World (x, y) of cell (0,0)'s lower corner.
    pub min: [f32; 2],
    pub polys: Vec<Poly>,
    /// `adj[p]` = neighbouring poly indices reachable across a shared portal edge.
    pub adj: Vec<Vec<u32>>,
    /// `col_polys[iy*nx + ix]` = poly indices whose rectangle covers cell `(ix, iy)`.
    col_polys: Vec<Vec<u32>>,
}

impl NavMesh {
    /// World bounds `(x0, y0, x1, y1)` of rectangle `p`.
    fn rect_bounds(&self, p: usize) -> (f32, f32, f32, f32) {
        let r = &self.polys[p];
        let cs = self.cell_size;
        (
            self.min[0] + r.ix as f32 * cs,
            self.min[1] + r.iy as f32 * cs,
            self.min[0] + (r.ix + r.w) as f32 * cs,
            self.min[1] + (r.iy + r.h) as f32 * cs,
        )
    }

    /// The point on rectangle `p` (at its height) closest in XY to `toward` — used to find
    /// where a stair bridge meets the rect's edge.
    fn closest_point(&self, p: usize, toward: [f32; 3]) -> [f32; 3] {
        let (x0, y0, x1, y1) = self.rect_bounds(p);
        [
            toward[0].clamp(x0, x1),
            toward[1].clamp(y0, y1),
            self.polys[p].oz,
        ]
    }

    /// Is there a real walkable stair between rects `a` and `b`? Samples several point pairs
    /// across their facing edges (a single closest-point test misses stairs whose climbable
    /// span isn't at the nearest corner — that was why the merged mesh under-bridged). Returns
    /// true if any sampled pair is connected by [`walkable_stair`] within `max_hdist`.
    fn stair_connects(&self, a: usize, b: usize, cm: &CollisionModel, max_hdist: f32) -> bool {
        let oza = self.polys[a].oz;
        let ozb = self.polys[b].oz;
        if (oza - ozb).abs() > STAIR_MAX {
            return false;
        }
        let (ax0, ay0, ax1, ay1) = self.rect_bounds(a);
        let (bx0, by0, bx1, by1) = self.rect_bounds(b);
        let (acx, acy) = ((ax0 + ax1) * 0.5, (ay0 + ay1) * 0.5);
        let (bcx, bcy) = ((bx0 + bx1) * 0.5, (by0 + by1) * 0.5);
        // Inset sample points a half-cell INTO each rect, so they sit on actual floor (cell
        // centers) rather than the boundary — walkable_stair from the very edge often fails.
        let inset = self.cell_size * 0.5;
        const N: usize = 4;
        let mut cands: Vec<([f32; 3], [f32; 3])> = Vec::new();

        let xov = (ax1.min(bx1)) - (ax0.max(bx0));
        if xov > 0.01 {
            // North/south neighbours: facing Y edges, sampled across the X overlap.
            let ya = if bcy > acy { ay1 - inset } else { ay0 + inset };
            let yb = if acy > bcy { by1 - inset } else { by0 + inset };
            let x0 = ax0.max(bx0);
            for k in 0..N {
                let x = x0 + xov * (k as f32 + 0.5) / N as f32;
                cands.push(([x, ya, oza], [x, yb, ozb]));
            }
        }
        let yov = (ay1.min(by1)) - (ay0.max(by0));
        if yov > 0.01 {
            // East/west neighbours: facing X edges, sampled across the Y overlap.
            let xa = if bcx > acx { ax1 - inset } else { ax0 + inset };
            let xb = if acx > bcx { bx1 - inset } else { bx0 + inset };
            let y0 = ay0.max(by0);
            for k in 0..N {
                let y = y0 + yov * (k as f32 + 0.5) / N as f32;
                cands.push(([xa, y, oza], [xb, y, ozb]));
            }
        }
        if cands.is_empty() {
            // No projection overlap (diagonal) — fall back to the closest points.
            cands.push((
                self.closest_point(a, [bcx, bcy, ozb]),
                self.closest_point(b, [acx, acy, oza]),
            ));
        }
        cands.into_iter().any(|(p, q)| {
            let hd = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt();
            if hd > max_hdist {
                return false;
            }
            let (lo, hi) = if p[2] <= q[2] { (p, q) } else { (q, p) };
            walkable_stair(cm, lo, hi)
        })
    }

    /// World position of rectangle `p`'s center (at its height).
    pub fn poly_center(&self, p: usize) -> [f32; 3] {
        let (x0, y0, x1, y1) = self.rect_bounds(p);
        [(x0 + x1) * 0.5, (y0 + y1) * 0.5, self.polys[p].oz]
    }

    /// The shared-edge **portal** between adjacent rects `a` and `b` (the overlap of their
    /// touching edges), as its two world endpoints (Z = average height). Panics if they do not
    /// share an edge (callers gate on [`Self::grid_adjacent`]).
    pub fn portal(&self, a: usize, b: usize) -> [[f32; 3]; 2] {
        let (ax0, ay0, ax1, ay1) = self.rect_bounds(a);
        let (bx0, by0, bx1, by1) = self.rect_bounds(b);
        let z = (self.polys[a].oz + self.polys[b].oz) * 0.5;
        let eps = 0.01;
        // Vertical shared edge (A east of B or B east of A).
        if (ax1 - bx0).abs() < eps {
            return [[ax1, ay0.max(by0), z], [ax1, ay1.min(by1), z]];
        }
        if (bx1 - ax0).abs() < eps {
            return [[ax0, ay0.max(by0), z], [ax0, ay1.min(by1), z]];
        }
        // Horizontal shared edge.
        if (ay1 - by0).abs() < eps {
            return [[ax0.max(bx0), ay1, z], [ax1.min(bx1), ay1, z]];
        }
        if (by1 - ay0).abs() < eps {
            return [[ax0.max(bx0), ay0, z], [ax1.min(bx1), ay0, z]];
        }
        panic!("portal() called on non-adjacent polys {a},{b}");
    }

    /// True if rects `a` and `b` share part of an edge (a real portal). Bridged links connect
    /// rects that don't touch; the funnel uses this to tell a portal from a bridge pinch.
    pub fn grid_adjacent(&self, a: usize, b: usize) -> bool {
        let (ax0, ay0, ax1, ay1) = self.rect_bounds(a);
        let (bx0, by0, bx1, by1) = self.rect_bounds(b);
        let eps = 0.01;
        let yov = ay1.min(by1) - ay0.max(by0);
        let xov = ax1.min(bx1) - ax0.max(bx0);
        let vtouch = ((ax1 - bx0).abs() < eps || (bx1 - ax0).abs() < eps) && yov > eps;
        let htouch = ((ay1 - by0).abs() < eps || (by1 - ay0).abs() < eps) && xov > eps;
        vtouch || htouch
    }

    /// Poly nearest to `pos`: searches the containing cell and its 8 neighbours and returns the
    /// rect minimising 3D distance to `pos` (so the correct floor of a stacked column is
    /// picked). `None` if no walkable poly is nearby.
    pub fn nearest_poly(&self, pos: [f32; 3]) -> Option<usize> {
        let cx = ((pos[0] - self.min[0]) / self.cell_size).floor() as i64;
        let cy = ((pos[1] - self.min[1]) / self.cell_size).floor() as i64;
        let mut best: Option<(usize, f32)> = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let ix = cx + dx;
                let iy = cy + dy;
                if ix < 0 || iy < 0 || ix >= self.nx as i64 || iy >= self.ny as i64 {
                    continue;
                }
                for &pid in &self.col_polys[iy as usize * self.nx + ix as usize] {
                    let c = self.poly_center(pid as usize);
                    let d = dist2(c, pos);
                    if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                        best = Some((pid as usize, d));
                    }
                }
            }
        }
        best.map(|(p, _)| p)
    }

    /// Connected components over the portal graph (for spawn-reachability validation).
    pub fn components(&self) -> Vec<Vec<usize>> {
        let n = self.polys.len();
        let mut comp = vec![usize::MAX; n];
        let mut out: Vec<Vec<usize>> = Vec::new();
        for start in 0..n {
            if comp[start] != usize::MAX {
                continue;
            }
            let id = out.len();
            let mut stack = vec![start];
            comp[start] = id;
            let mut members = Vec::new();
            while let Some(p) = stack.pop() {
                members.push(p);
                for &q in &self.adj[p] {
                    if comp[q as usize] == usize::MAX {
                        comp[q as usize] = id;
                        stack.push(q as usize);
                    }
                }
            }
            out.push(members);
        }
        out
    }

    /// Build the navmesh: greedy-merge walkable cell-spans into z-coherent rectangles, then
    /// connect rects that share an edge and are within `walkable_climb`.
    pub fn build(hf: &Heightfield, walkable_climb: f32) -> Self {
        let nx = hf.nx;
        let ny = hf.ny;
        let cols = &hf.columns;
        // Per-(cell, span) assignment flags.
        let mut assigned: Vec<Vec<bool>> = cols.iter().map(|c| vec![false; c.len()]).collect();

        // Find an unassigned span in cell `ci` whose Z is within `walkable_climb` of `z`.
        let find = |ci: usize, z: f32, assigned: &[Vec<bool>]| -> Option<usize> {
            cols[ci]
                .iter()
                .enumerate()
                .find(|(si, &cz)| !assigned[ci][*si] && (cz - z).abs() <= walkable_climb)
                .map(|(si, _)| si)
        };

        let mut polys: Vec<Poly> = Vec::new();
        let mut col_polys: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];

        for iy0 in 0..ny {
            for ix0 in 0..nx {
                let ci0 = iy0 * nx + ix0;
                for si0 in 0..cols[ci0].len() {
                    if assigned[ci0][si0] {
                        continue;
                    }
                    let seed_z = cols[ci0][si0];

                    // Grow width along +x while the next column has a matchable span.
                    let mut w = 1;
                    while ix0 + w < nx && find(iy0 * nx + ix0 + w, seed_z, &assigned).is_some() {
                        w += 1;
                    }
                    // Grow height along +y while the WHOLE next row is matchable.
                    let mut h = 1;
                    'rows: while iy0 + h < ny {
                        for dx in 0..w {
                            if find((iy0 + h) * nx + ix0 + dx, seed_z, &assigned).is_none() {
                                break 'rows;
                            }
                        }
                        h += 1;
                    }

                    // Claim the rectangle's spans.
                    let pid = polys.len() as u32;
                    for dy in 0..h {
                        for dx in 0..w {
                            let ci = (iy0 + dy) * nx + ix0 + dx;
                            if let Some(si) = find(ci, seed_z, &assigned) {
                                assigned[ci][si] = true;
                                col_polys[ci].push(pid);
                            }
                        }
                    }
                    polys.push(Poly {
                        ix: ix0 as u32,
                        iy: iy0 as u32,
                        w: w as u32,
                        h: h as u32,
                        oz: seed_z,
                    });
                }
            }
        }

        let mut mesh = Self {
            cell_size: hf.cell_size,
            nx,
            ny,
            min: hf.min,
            polys,
            adj: Vec::new(),
            col_polys,
        };
        mesh.adj = mesh.build_adjacency(walkable_climb);
        mesh
    }

    /// Portal adjacency: for each rect, collect distinct rects covering the cells just outside
    /// its four edges whose height is within `walkable_climb`.
    fn build_adjacency(&self, walkable_climb: f32) -> Vec<Vec<u32>> {
        use std::collections::HashSet;
        let nx = self.nx;
        (0..self.polys.len())
            .map(|p| {
                let r = self.polys[p];
                let (ix, iy, w, h) = (r.ix as usize, r.iy as usize, r.w as usize, r.h as usize);
                let mut set: HashSet<u32> = HashSet::new();
                let consider = |ci: usize, set: &mut HashSet<u32>| {
                    for &q in &self.col_polys[ci] {
                        if q as usize != p
                            && (self.polys[q as usize].oz - r.oz).abs() <= walkable_climb
                        {
                            set.insert(q);
                        }
                    }
                };
                if ix + w < nx {
                    for j in iy..iy + h {
                        consider(j * nx + ix + w, &mut set);
                    }
                }
                if ix > 0 {
                    for j in iy..iy + h {
                        consider(j * nx + ix - 1, &mut set);
                    }
                }
                if iy + h < self.ny {
                    for i in ix..ix + w {
                        consider((iy + h) * nx + i, &mut set);
                    }
                }
                if iy > 0 {
                    for i in ix..ix + w {
                        consider((iy - 1) * nx + i, &mut set);
                    }
                }
                set.into_iter().collect()
            })
            .collect()
    }

    /// Stitch walkable fragments the rectangle adjacency missed — Q2 staircases/ramps whose
    /// treads put the next rect more than `walkable_climb` away in height. Mirrors
    /// `NavGraph::bridge_components`: for each rect outside the largest component, find the
    /// nearest rect in a *different* component within `max_hdist` + `STAIR_MAX` connected by a
    /// real [`walkable_stair`], and add a portal. Returns directed portals added.
    pub fn bridge_components(&mut self, cm: &CollisionModel, max_hdist: f32) -> usize {
        let mut added = 0usize;

        for _pass in 0..6 {
            let comps = self.components();
            if comps.len() == 1 {
                break;
            }
            let mut comp_of = vec![usize::MAX; self.polys.len()];
            for (ci, c) in comps.iter().enumerate() {
                for &p in c {
                    comp_of[p] = ci;
                }
            }
            let largest = (0..comps.len()).max_by_key(|&i| comps[i].len()).unwrap();

            // For each rect outside the largest component, find the best stair-connectable rect
            // in another component. Test `walkable_stair` between the two rects' CLOSEST points
            // (the stair connects at their facing edges, not their far-apart centers).
            let bridges: Vec<(u32, u32)> = (0..self.polys.len())
                .into_par_iter()
                .filter(|&pid| comp_of[pid] != largest)
                .filter_map(|pid| {
                    let pcenter = self.poly_center(pid);
                    // Prefer the nearest other-component rect that a real stair connects to.
                    let mut best: Option<(u32, f32)> = None;
                    for q in 0..self.polys.len() {
                        if comp_of[q] == comp_of[pid] {
                            continue;
                        }
                        let qc = self.poly_center(q);
                        let hd2 = (qc[0] - pcenter[0]).powi(2) + (qc[1] - pcenter[1]).powi(2);
                        if best.map(|(_, s)| hd2 >= s).unwrap_or(false) {
                            continue; // already have a closer candidate; skip the stair test
                        }
                        if self.stair_connects(pid, q, cm, max_hdist) {
                            best = Some((q as u32, hd2));
                        }
                    }
                    best.map(|(q, _)| (pid as u32, q))
                })
                .collect();

            if bridges.is_empty() {
                break;
            }
            for (a, b) in bridges {
                if !self.adj[a as usize].contains(&b) {
                    self.adj[a as usize].push(b);
                    added += 1;
                }
                if !self.adj[b as usize].contains(&a) {
                    self.adj[b as usize].push(a);
                    added += 1;
                }
            }
        }
        added
    }
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat 4×1 strip merges into ONE rectangle.
    #[test]
    fn flat_strip_merges_to_one_rect() {
        let hf = Heightfield {
            cell_size: 16.0,
            nx: 4,
            ny: 1,
            min: [0.0, 0.0],
            columns: vec![vec![24.0]; 4],
        };
        let m = NavMesh::build(&hf, 18.0);
        assert_eq!(m.polys.len(), 1);
        assert_eq!(m.polys[0].w, 4);
        assert_eq!(m.components().len(), 1);
    }

    /// An L-shape merges into two rectangles that share an edge (one portal, one component).
    #[test]
    fn l_shape_two_rects_share_a_portal() {
        // bottom row y=0 (x 0..3) + a cell stacked at (0,1): the greedy merge makes
        // rect A = the 3×1 bottom row, rect B = the single (0,1) cell, sharing the edge at y=16.
        let nx = 3;
        let ny = 2;
        let mut columns = vec![Vec::new(); nx * ny];
        for ix in 0..nx {
            columns[ix] = vec![24.0]; // bottom row
        }
        columns[nx] = vec![24.0]; // cell (0,1)
        let hf = Heightfield {
            cell_size: 16.0,
            nx,
            ny,
            min: [0.0, 0.0],
            columns,
        };
        let m = NavMesh::build(&hf, 18.0);
        assert_eq!(m.polys.len(), 2);
        assert_eq!(m.components().len(), 1);
        // They share the horizontal edge at y=16 over x[0..16].
        assert!(m.grid_adjacent(0, 1));
        let p = m.portal(0, 1);
        assert!((p[0][1] - 16.0).abs() < 0.1 && (p[1][1] - 16.0).abs() < 0.1);
    }

    /// A big height gap keeps stacked floors as separate components.
    #[test]
    fn stacked_floors_split_components() {
        let hf = Heightfield {
            cell_size: 16.0,
            nx: 2,
            ny: 1,
            min: [0.0, 0.0],
            columns: vec![vec![24.0, 224.0], vec![24.0, 224.0]],
        };
        let m = NavMesh::build(&hf, 18.0);
        // Two rects (lower strip, upper strip), two components.
        assert_eq!(m.polys.len(), 2);
        assert_eq!(m.components().len(), 2);
    }

    #[test]
    fn nearest_poly_picks_closest_floor_in_a_stack() {
        let hf = Heightfield {
            cell_size: 16.0,
            nx: 2,
            ny: 1,
            min: [0.0, 0.0],
            columns: vec![vec![24.0, 224.0], vec![24.0, 224.0]],
        };
        let m = NavMesh::build(&hf, 18.0);
        let p = m.nearest_poly([8.0, 8.0, 220.0]).unwrap();
        assert!((m.poly_center(p)[2] - 224.0).abs() < 1.0);
    }
}
