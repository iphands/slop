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

use crate::collision::{CollisionModel, MASK_SOLID};
use crate::navgraph::{STAIR_MAX, STEP};
use crate::navmesh::heightfield::Heightfield;

/// Strict pmove-accurate climb check: can the bot actually *walk* from `lo` (lower origin) up
/// to `hi` (higher origin), stepping at most `STEP` (18u) per increment with a real tread to
/// stand on at each? This rejects `walkable_stair`'s false positives — a tall ledge/wall where
/// the only "floor below the probe" is the bottom, with no intermediate tread (a one-way drop,
/// not a climb). Walks the XY line in 8u increments; at each, the floor must rise ≤ STEP from
/// the previous and the hull must fit standing. Also validates flat gap bridges (rise ≈ 0).
fn climbable_walk(cm: &CollisionModel, lo: [f32; 3], hi: [f32; 3]) -> bool {
    let zero = [0.0f32; 3];
    let dx = hi[0] - lo[0];
    let dy = hi[1] - lo[1];
    let hd = (dx * dx + dy * dy).sqrt();
    let steps = ((hd / 8.0).ceil() as usize).max(1);
    let mut prev_f = lo[2] - 24.0; // current floor surface (origin = floor + 24)
    for i in 1..=steps {
        let f = i as f32 / steps as f32;
        let x = lo[0] + dx * f;
        let y = lo[1] + dy * f;
        // The next floor may rise at most STEP above the current (a step up) or drop (down ok).
        // Probing from just above the max step height rejects a taller wall as `startsolid`.
        let top = [x, y, prev_f + STEP + 2.0];
        let bot = [x, y, prev_f - 96.0];
        let tr = cm.trace(&top, &bot, &zero, &zero, MASK_SOLID);
        if tr.startsolid || tr.fraction >= 1.0 {
            return false; // wall (probe starts inside the tall step) or no floor (gap)
        }
        let surf = tr.endpos[2];
        if surf - prev_f > STEP + 1.0 {
            return false; // step too tall to climb
        }
        // (No full-hull fit test here: the heightfield admits near-wall cells without erosion,
        // and the `startsolid` above already rejects a wall in the path — a hull test would
        // wrongly drop every near-wall walkable cell.)
        prev_f = surf;
    }
    (prev_f - (hi[2] - 24.0)).abs() < STEP + 4.0
}

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
    /// Parallel to `col_polys`: the actual floor-surface Z each covering rect uses at this cell
    /// (a rect spans ≤ `walkable_climb`, so its per-cell floor differs from its seed `oz`).
    /// Adjacency compares these real cell floors across a shared edge, not the rects' seed `oz`.
    col_floor: Vec<Vec<f32>>,
    /// For each bridged (non-edge-sharing) rect pair `(a, b)`, the two WALKABLE cell centers
    /// the stair connects — `[point_on_a, point_on_b]`. The funnel routes the path through both
    /// (a's side, then b's), so the bot walks the stair surface. Storing a single midpoint
    /// instead put the pinch INSIDE the step's solid (bot aimed into a wall and wedged).
    bridge_points: std::collections::HashMap<(u32, u32), [[f32; 3]; 2]>,
    /// `ledge[p]` = rect `p` sits next to a drop (a place to fall off), set from `find_drops`.
    /// Narrow ledge rects are pinned by the funnel so the bot doesn't get cut off the ledge.
    ledge: Vec<bool>,
}

impl NavMesh {
    /// An empty navmesh (no polygons): every `path` query returns `None`. Useful as a
    /// degenerate stand-in and for unit tests that drive a [`crate::NavMesh`]-backed component
    /// without parsing a BSP.
    pub fn empty() -> Self {
        Self {
            cell_size: 1.0,
            nx: 0,
            ny: 0,
            min: [0.0, 0.0],
            polys: Vec::new(),
            adj: Vec::new(),
            col_polys: Vec::new(),
            col_floor: Vec::new(),
            bridge_points: std::collections::HashMap::new(),
            ledge: Vec::new(),
        }
    }

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

    /// World center of cell `(ix, iy)` at height `z`.
    fn cell_center(&self, ix: usize, iy: usize, z: f32) -> [f32; 3] {
        [
            self.min[0] + (ix as f32 + 0.5) * self.cell_size,
            self.min[1] + (iy as f32 + 0.5) * self.cell_size,
            z,
        ]
    }

    /// True if rect `p` is a narrow **flat** ledge (≤3 cells wide, and all its neighbours are at
    /// roughly its own height — so it's a thin walkway, not a stair step whose neighbours differ
    /// by ~STEP). The funnel routes through these polys' centers instead of straightening across
    /// them, so the bot follows a winding thin ledge (the RL route) rather than cutting off it —
    /// while stairs still straighten normally (pinning them regressed spawn-to-spawn).
    pub fn is_narrow_ledge(&self, p: usize) -> bool {
        self.polys[p].w.min(self.polys[p].h) <= 3 && self.ledge.get(p).copied().unwrap_or(false)
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
    /// rect covering that cell whose **actual floor at the cell** is closest to `pos.z` (the
    /// containing cell is preferred over neighbours). Ranking by the rect's per-cell floor — not
    /// its far-away center — picks the right floor of a stacked column even for big rects.
    /// `None` if no walkable poly is nearby.
    pub fn nearest_poly(&self, pos: [f32; 3]) -> Option<usize> {
        let cx = ((pos[0] - self.min[0]) / self.cell_size).floor() as i64;
        let cy = ((pos[1] - self.min[1]) / self.cell_size).floor() as i64;
        let cell2 = self.cell_size * self.cell_size;
        let mut best: Option<(usize, f32)> = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let ix = cx + dx;
                let iy = cy + dy;
                if ix < 0 || iy < 0 || ix >= self.nx as i64 || iy >= self.ny as i64 {
                    continue;
                }
                let nci = iy as usize * self.nx + ix as usize;
                let cell_pen = (dx * dx + dy * dy) as f32 * cell2; // prefer the containing cell
                for (k, &pid) in self.col_polys[nci].iter().enumerate() {
                    let vd = self.col_floor[nci][k] - pos[2];
                    let score = cell_pen + vd * vd;
                    if best.map(|(_, bd)| score < bd).unwrap_or(true) {
                        best = Some((pid as usize, score));
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
    /// `cm` (when `Some`) validates each adjacency hop with a collision trace so a thin wall
    /// between two step-apart cells doesn't become a false portal. Unit tests pass `None`.
    pub fn build(hf: &Heightfield, walkable_climb: f32, cm: Option<&CollisionModel>) -> Self {
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
        let mut col_floor: Vec<Vec<f32>> = vec![Vec::new(); nx * ny];

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

                    // Claim the rectangle's spans, recording each cell's actual floor Z.
                    let pid = polys.len() as u32;
                    for dy in 0..h {
                        for dx in 0..w {
                            let ci = (iy0 + dy) * nx + ix0 + dx;
                            if let Some(si) = find(ci, seed_z, &assigned) {
                                assigned[ci][si] = true;
                                col_polys[ci].push(pid);
                                col_floor[ci].push(cols[ci][si]);
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
            col_floor,
            bridge_points: std::collections::HashMap::new(),
            ledge: Vec::new(),
        };
        mesh.adj = mesh.build_adjacency(cm);
        mesh.ledge = vec![false; mesh.polys.len()];
        mesh
    }

    /// Wire one-way **drop links** (from [`Heightfield::find_drops`], computed on the FULL
    /// heightfield) into this eroded mesh: each `(edge, landing)` connects the eroded rect
    /// nearest the ledge edge to the rect nearest the landing, with a directed high→low edge.
    /// This reaches drop-only spots (q2dm1's z=920 RL ledge, dropped onto from the z=1256
    /// grenade-launcher) without keeping the fall-prone ledge cells in the walkable mesh. The
    /// funnel routes through the recorded `[edge, landing]` points (the bot walks off the edge).
    pub fn add_drops(&mut self, drops: &[([f32; 3], [f32; 3])]) {
        for &(edge, land) in drops {
            let (Some(a), Some(b)) = (self.nearest_poly(edge), self.nearest_poly(land)) else {
                continue;
            };
            self.ledge[a] = true; // the high rect sits on a drop edge (a place to fall off)
            if a != b
                && (self.polys[a].oz - self.polys[b].oz) > STEP
                && !self.adj[a].contains(&(b as u32))
            {
                self.adj[a].push(b as u32); // directed: high → low only
                self.bridge_points
                    .entry((a as u32, b as u32))
                    .or_insert([edge, land]);
            }
        }
    }

    /// The two walkable cell centers a bridge connects, `[on_a, on_b]`, if `(a, b)` is bridged.
    pub fn bridge_point(&self, a: usize, b: usize) -> Option<[[f32; 3]; 2]> {
        self.bridge_points.get(&(a as u32, b as u32)).copied()
    }

    /// Portal adjacency, **cell-step based**: two rects are adjacent iff some 4-neighbour cell
    /// pair across their boundary has floor surfaces within one `STEP` (18u) — a height the bot
    /// can step over. This uses the real per-cell floors (`col_floor`), not the rects' seed
    /// `oz`, so a staircase's per-step rects (whose seeds can differ by >18u) connect tread to
    /// tread, while a >18u ledge between two cells does not (it needs a drop/bridge).
    fn build_adjacency(&self, cm: Option<&CollisionModel>) -> Vec<Vec<u32>> {
        use std::collections::HashSet;
        let nx = self.nx;
        let ny = self.ny;
        let mut adj: Vec<HashSet<u32>> = vec![HashSet::new(); self.polys.len()];
        for ci in 0..nx * ny {
            if self.col_polys[ci].is_empty() {
                continue;
            }
            let ix = ci % nx;
            let iy = ci / nx;
            for (ri, &r) in self.col_polys[ci].iter().enumerate() {
                let fr = self.col_floor[ci][ri];
                for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                    let nxi = ix as i64 + dx;
                    let nyi = iy as i64 + dy;
                    if nxi < 0 || nyi < 0 || nxi >= nx as i64 || nyi >= ny as i64 {
                        continue;
                    }
                    let nci = nyi as usize * nx + nxi as usize;
                    for (qi, &q) in self.col_polys[nci].iter().enumerate() {
                        if q == r || adj[r as usize].contains(&q) {
                            continue; // self, or already connected via another cell hop
                        }
                        let fq = self.col_floor[nci][qi];
                        if (fq - fr).abs() > STEP + 1.0 {
                            continue;
                        }
                        // Validate the actual cell hop with collision (when available): two
                        // walkable cells a step apart can still have a thin WALL between them —
                        // climbable_walk catches it. Without cm (unit tests) trust the floor diff.
                        let ok = cm.is_none_or(|c| {
                            // col_floor stores ORIGIN z (floor + 24), which is what climbable_walk
                            // expects — pass it directly, do NOT add another 24.
                            let a = self.cell_center(ix, iy, fr);
                            let b = self.cell_center(nxi as usize, nyi as usize, fq);
                            let (lo, hi) = if fr <= fq { (a, b) } else { (b, a) };
                            climbable_walk(c, lo, hi)
                        });
                        if ok {
                            adj[r as usize].insert(q);
                            adj[q as usize].insert(r);
                        }
                    }
                }
            }
        }
        adj.into_iter().map(|s| s.into_iter().collect()).collect()
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
            let ring = (max_hdist / self.cell_size).ceil() as i64;
            let nx = self.nx;
            let ny = self.ny;

            // Cell-granular bridge (matches the per-cell version that connected everything):
            // for every cell of a non-largest-component rect, ring-search nearby cells; if a
            // real walkable_stair connects the two CELL CENTERS, bridge the two RECTS. Rect-edge
            // sampling under-bridged because the climbable span isn't always at a rect edge.
            let bridges: Vec<(u32, u32, [f32; 3], [f32; 3])> = (0..nx * ny)
                .into_par_iter()
                .flat_map_iter(|ci| {
                    let ix = ci % nx;
                    let iy = ci / nx;
                    let mut out: Vec<(u32, u32, [f32; 3], [f32; 3])> = Vec::new();
                    for &r in &self.col_polys[ci] {
                        if comp_of[r as usize] == largest {
                            continue;
                        }
                        let rz = self.polys[r as usize].oz;
                        let c0 = self.cell_center(ix, iy, rz);
                        for dy in -ring..=ring {
                            for dx in -ring..=ring {
                                let nxi = ix as i64 + dx;
                                let nyi = iy as i64 + dy;
                                if nxi < 0 || nyi < 0 || nxi >= nx as i64 || nyi >= ny as i64 {
                                    continue;
                                }
                                for &q in &self.col_polys[nyi as usize * nx + nxi as usize] {
                                    if comp_of[q as usize] == comp_of[r as usize] {
                                        continue;
                                    }
                                    let qz = self.polys[q as usize].oz;
                                    if (qz - rz).abs() > STAIR_MAX {
                                        continue;
                                    }
                                    let c1 = self.cell_center(nxi as usize, nyi as usize, qz);
                                    let hd =
                                        ((c1[0] - c0[0]).powi(2) + (c1[1] - c0[1]).powi(2)).sqrt();
                                    // Reject too-steep links. A real Q2 staircase rises ~16u per
                                    // tread over a ≥ comparable run (slope ≲ 1). walkable_stair
                                    // false-positives on a tall ledge/wall climbed over a short
                                    // run (it finds the lower floor "within a step below" each
                                    // probe even with no real tread), so the bot wedges at the
                                    // base trying to climb. Require a stair-like run: hd ≥ 1.5·dz.
                                    if hd > max_hdist || hd < (qz - rz).abs() {
                                        continue;
                                    }
                                    let (lo, hi) = if rz <= qz { (c0, c1) } else { (c1, c0) };
                                    if climbable_walk(cm, lo, hi) {
                                        // Carry BOTH walkable cell centers (r's side, q's side).
                                        out.push((r, q, c0, c1));
                                    }
                                }
                            }
                        }
                    }
                    out
                })
                .collect();

            if bridges.is_empty() {
                break;
            }
            for (a, b, ca, cb) in bridges {
                if !self.adj[a as usize].contains(&b) {
                    self.adj[a as usize].push(b);
                    added += 1;
                }
                if !self.adj[b as usize].contains(&a) {
                    self.adj[b as usize].push(a);
                    added += 1;
                }
                self.bridge_points.entry((a, b)).or_insert([ca, cb]);
                self.bridge_points.entry((b, a)).or_insert([cb, ca]);
            }
        }
        added
    }
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
        let m = NavMesh::build(&hf, 18.0, None);
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
        for c in columns.iter_mut().take(nx) {
            *c = vec![24.0]; // bottom row
        }
        columns[nx] = vec![24.0]; // cell (0,1)
        let hf = Heightfield {
            cell_size: 16.0,
            nx,
            ny,
            min: [0.0, 0.0],
            columns,
        };
        let m = NavMesh::build(&hf, 18.0, None);
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
        let m = NavMesh::build(&hf, 18.0, None);
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
        let m = NavMesh::build(&hf, 18.0, None);
        let p = m.nearest_poly([8.0, 8.0, 220.0]).unwrap();
        assert!((m.poly_center(p)[2] - 224.0).abs() < 1.0);
    }
}
