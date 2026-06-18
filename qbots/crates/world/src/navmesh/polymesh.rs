//! Phase 2 — walkable polygons + portal adjacency, built from the [`Heightfield`].
//!
//! The simplest *correct* navmesh from a voxel grid: each walkable cell-span is one convex
//! quad polygon, and 4-neighbour cell-spans within `walkable_climb` of each other share a
//! **portal** (their common edge). That is enough for the two things a navmesh must deliver:
//! polygon A* (Phase 3) and the funnel, which pulls a path taut through the portal edges
//! regardless of how many polygons it crosses — so movement quality is density-independent
//! and (thanks to the hull-eroded heightfield) stays off walls. Merging cells into larger
//! rectangles is a perf optimisation, not a correctness one, and is left as a follow-up.
//!
//! Connectivity within `walkable_climb` (Q2 `STEP` = 18) means a staircase's per-step cells
//! link up, while stacked floors (z apart by ≫ STEP) stay separate components.

use crate::navmesh::heightfield::Heightfield;

/// One walkable quad: a single cell at grid `(ix, iy)` whose player-origin height is `oz`.
/// The four world corners are derived on demand from the grid + cell size (the quad is flat
/// at `oz`); storing only the indices keeps the mesh compact.
#[derive(Debug, Clone, Copy)]
pub struct Poly {
    pub ix: u32,
    pub iy: u32,
    /// Player-origin Z at this cell (floor surface + 24).
    pub oz: f32,
}

/// A polygon navmesh: walkable quads + portal adjacency, plus the grid index needed for
/// `nearest_poly` and A*. Geometry is `[f32; 3]` (the crate stays glam-free).
pub struct NavMesh {
    pub cell_size: f32,
    pub nx: usize,
    pub ny: usize,
    /// World (x, y) of cell (0,0)'s lower corner; cell `(ix,iy)` spans
    /// `[min.x + ix*cs, min.x + (ix+1)*cs] × [min.y + iy*cs, …]`.
    pub min: [f32; 2],
    pub polys: Vec<Poly>,
    /// `adj[p]` = neighbouring poly indices reachable across a shared portal edge.
    pub adj: Vec<Vec<u32>>,
    /// `col_polys[iy*nx + ix]` = poly indices whose cell is `(ix, iy)` (one per span level).
    col_polys: Vec<Vec<u32>>,
}

impl NavMesh {
    /// World position of poly `p`'s center (cell midpoint at its height).
    pub fn poly_center(&self, p: usize) -> [f32; 3] {
        let poly = &self.polys[p];
        [
            self.min[0] + (poly.ix as f32 + 0.5) * self.cell_size,
            self.min[1] + (poly.iy as f32 + 0.5) * self.cell_size,
            poly.oz,
        ]
    }

    /// The shared-edge **portal** between adjacent polys `a` and `b`, as its two world
    /// endpoints (Z = average height). Panics if they are not 4-neighbours. The endpoints
    /// are returned in a fixed grid order; the funnel orients them per travel direction.
    pub fn portal(&self, a: usize, b: usize) -> [[f32; 3]; 2] {
        let pa = &self.polys[a];
        let pb = &self.polys[b];
        let cs = self.cell_size;
        let z = (pa.oz + pb.oz) * 0.5;
        let dx = pb.ix as i64 - pa.ix as i64;
        let dy = pb.iy as i64 - pa.iy as i64;
        // The shared edge is the boundary of cell `a` toward `b`.
        let x0 = self.min[0] + pa.ix as f32 * cs;
        let y0 = self.min[1] + pa.iy as f32 * cs;
        match (dx, dy) {
            (1, 0) => [[x0 + cs, y0, z], [x0 + cs, y0 + cs, z]], // east edge
            (-1, 0) => [[x0, y0, z], [x0, y0 + cs, z]],          // west edge
            (0, 1) => [[x0, y0 + cs, z], [x0 + cs, y0 + cs, z]], // north edge
            (0, -1) => [[x0, y0, z], [x0 + cs, y0, z]],          // south edge
            _ => panic!("portal() called on non-adjacent polys {a},{b}"),
        }
    }

    /// Poly nearest to `pos`: searches the containing cell and its 8 neighbours and returns
    /// the poly minimising 3D distance to `pos` (so the correct floor of a stacked column is
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

    /// Build the navmesh from a heightfield: one quad per walkable cell-span, portals between
    /// 4-neighbour spans within `walkable_climb`.
    pub fn build(hf: &Heightfield, walkable_climb: f32) -> Self {
        let nx = hf.nx;
        let ny = hf.ny;
        let mut polys: Vec<Poly> = Vec::new();
        let mut col_polys: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];

        for iy in 0..ny {
            for ix in 0..nx {
                let ci = iy * nx + ix;
                for &oz in &hf.columns[ci] {
                    let pid = polys.len() as u32;
                    polys.push(Poly {
                        ix: ix as u32,
                        iy: iy as u32,
                        oz,
                    });
                    col_polys[ci].push(pid);
                }
            }
        }

        let mut adj: Vec<Vec<u32>> = vec![Vec::new(); polys.len()];
        for pid in 0..polys.len() {
            let p = polys[pid];
            for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let nxi = p.ix as i64 + dx;
                let nyi = p.iy as i64 + dy;
                if nxi < 0 || nyi < 0 || nxi >= nx as i64 || nyi >= ny as i64 {
                    continue;
                }
                let nci = nyi as usize * nx + nxi as usize;
                for &npid in &col_polys[nci] {
                    if (polys[npid as usize].oz - p.oz).abs() <= walkable_climb {
                        adj[pid].push(npid);
                    }
                }
            }
        }

        Self {
            cell_size: hf.cell_size,
            nx,
            ny,
            min: hf.min,
            polys,
            adj,
            col_polys,
        }
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

    /// 2×1 flat strip of two cells → two polys joined by one portal, one component.
    fn strip() -> NavMesh {
        let hf = Heightfield {
            cell_size: 8.0,
            nx: 2,
            ny: 1,
            min: [0.0, 0.0],
            columns: vec![vec![24.0], vec![24.0]],
        };
        NavMesh::build(&hf, 18.0)
    }

    #[test]
    fn adjacent_flat_cells_share_a_portal_and_one_component() {
        let m = strip();
        assert_eq!(m.polys.len(), 2);
        assert_eq!(m.adj[0], vec![1]);
        assert_eq!(m.adj[1], vec![0]);
        assert_eq!(m.components().len(), 1);
    }

    #[test]
    fn portal_between_east_neighbors_is_the_shared_edge() {
        let m = strip();
        // Cell 0 spans x[0..8], cell 1 x[8..16]; shared edge is x=8, y[0..8].
        let p = m.portal(0, 1);
        assert_eq!(p[0], [8.0, 0.0, 24.0]);
        assert_eq!(p[1], [8.0, 8.0, 24.0]);
    }

    #[test]
    fn big_height_gap_splits_components() {
        // Two stacked floors in the same column-pair, 200u apart → not connected.
        let hf = Heightfield {
            cell_size: 8.0,
            nx: 2,
            ny: 1,
            min: [0.0, 0.0],
            columns: vec![vec![24.0, 224.0], vec![24.0, 224.0]],
        };
        let m = NavMesh::build(&hf, 18.0);
        assert_eq!(m.polys.len(), 4);
        assert_eq!(m.components().len(), 2); // lower floor, upper floor
    }

    #[test]
    fn nearest_poly_picks_the_closest_floor_in_a_stack() {
        let hf = Heightfield {
            cell_size: 8.0,
            nx: 2,
            ny: 1,
            min: [0.0, 0.0],
            columns: vec![vec![24.0, 224.0], vec![24.0, 224.0]],
        };
        let m = NavMesh::build(&hf, 18.0);
        // Query near the upper floor of cell 0.
        let p = m.nearest_poly([4.0, 4.0, 220.0]).unwrap();
        assert!((m.poly_center(p)[2] - 224.0).abs() < 1.0);
    }
}
