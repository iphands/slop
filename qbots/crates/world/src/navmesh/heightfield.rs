//! Phase 1 — voxel heightfield of walkable spans.
//!
//! Rasterizes the map into a grid of columns at `cell_size`. Each column holds the
//! player-origin Z of every surface with a solid floor and player-height **headroom**
//! (multi-level: Q2 maps stack floors). Built on `CollisionModel` traces — the same
//! primitive the waypoint graph uses — so it needs no game DLL.
//!
//! `column_floors` does NOT erode (a per-cell hull-fit test severs Q2 doorways — a 32u door's
//! only hull-fit point is its exact center, unsampled by any grid). Wall clearance is instead a
//! separate [`Heightfield::erode`] pass: a Recast-style distance field that drops the near-wall
//! ring (where the hull jams) while keeping passage centerlines, run at a fine `cell_size` so
//! doorways survive. The funnel's portal inset + `pursue_target_safe` are further backstops.

use rayon::prelude::*;

use crate::collision::{CollisionModel, MASK_SOLID, MASK_WATER};
use crate::navgraph::STEP;

/// Build-time voxelization parameters. `cell_size` is a *resolution* knob (finer = more
/// build cost, NOT worse navigation). `walkable_climb`/`agent_radius` are consumed by later
/// phases (span adjacency, cache fingerprint) but recorded here so one struct keys the build.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoxelParams {
    /// Horizontal grid spacing in units. Default 8.
    pub cell_size: f32,
    /// Max floor-to-floor rise (units) two adjacent cells may differ by and still be a
    /// walkable step (Q2 `STEP` = 18). Drives span adjacency in Phase 2.
    pub walkable_climb: f32,
    /// Agent (player hull) radius in units (16). Erosion is implicit in the hull-fit test;
    /// kept for the cache fingerprint and documentation.
    pub agent_radius: f32,
}

impl Default for VoxelParams {
    fn default() -> Self {
        Self {
            cell_size: 8.0,
            walkable_climb: STEP,
            agent_radius: 16.0,
        }
    }
}

/// A grid of walkable spans. `columns[iy * nx + ix]` is the sorted list of player-origin Z
/// values (floor surface + 24) the hull fits on in that cell — possibly several, for stacked
/// floors. Empty = no walkable surface in that column.
pub struct Heightfield {
    pub cell_size: f32,
    pub nx: usize,
    pub ny: usize,
    /// World (x, y) of the *center* of cell (0, 0) is `min[k] + 0.5 * cell_size`.
    pub min: [f32; 2],
    pub columns: Vec<Vec<f32>>,
}

impl Heightfield {
    /// World-space center of cell `(ix, iy)` at a given origin-Z.
    pub fn cell_center(&self, ix: usize, iy: usize, oz: f32) -> [f32; 3] {
        [
            self.min[0] + (ix as f32 + 0.5) * self.cell_size,
            self.min[1] + (iy as f32 + 0.5) * self.cell_size,
            oz,
        ]
    }

    /// Total walkable spans across all columns (a stacked column counts each floor).
    pub fn walkable_span_count(&self) -> usize {
        self.columns.iter().map(Vec::len).sum()
    }

    /// Columns that have at least one walkable span.
    pub fn walkable_column_count(&self) -> usize {
        self.columns.iter().filter(|c| !c.is_empty()).count()
    }

    /// **Agent-radius erosion** (Recast-style distance field). Computes each walkable cell-span's
    /// distance (in cells) to the nearest border (a side with no span within `STEP` — a wall or
    /// drop, or the map edge), via multi-source BFS over the step-connected span graph, then
    /// removes spans closer than `radius_cells`. This drops the near-wall ring where the player
    /// hull would jam in solid, while a passage's centerline (max distance) survives — so the bot
    /// is routed through hull-fitting space, not pinned to a wall. Use a small `radius_cells`
    /// (≈1) so 32u-wide Q2 doorways (exactly the hull width) keep a centerline.
    #[allow(clippy::needless_range_loop)] // parallel cell/span arrays read the same index
    pub fn erode(&mut self, radius_cells: u32) {
        use std::collections::VecDeque;
        if radius_cells == 0 {
            return;
        }
        let nx = self.nx;
        let ny = self.ny;
        let mut dist: Vec<Vec<u32>> = self
            .columns
            .iter()
            .map(|c| vec![u32::MAX; c.len()])
            .collect();
        let mut q: VecDeque<(usize, usize)> = VecDeque::new();
        let dirs = [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)];

        // A span is a border if any side lacks a span within STEP (a wall OR a ledge/drop edge).
        // We erode BOTH so the eroded mesh sits off ledge edges too (bots don't drift off and
        // fall); intentional drops are re-added as off-mesh links from the full heightfield.
        for ci in 0..nx * ny {
            let ix = (ci % nx) as i64;
            let iy = (ci / nx) as i64;
            for (k, &z) in self.columns[ci].iter().enumerate() {
                let border = dirs.iter().any(|&(dx, dy)| {
                    let (nxi, nyi) = (ix + dx, iy + dy);
                    if nxi < 0 || nyi < 0 || nxi >= nx as i64 || nyi >= ny as i64 {
                        return true;
                    }
                    let nci = nyi as usize * nx + nxi as usize;
                    !self.columns[nci].iter().any(|&nz| (nz - z).abs() <= STEP)
                });
                if border {
                    dist[ci][k] = 0;
                    q.push_back((ci, k));
                }
            }
        }

        while let Some((ci, k)) = q.pop_front() {
            let d = dist[ci][k];
            let z = self.columns[ci][k];
            let ix = (ci % nx) as i64;
            let iy = (ci / nx) as i64;
            for &(dx, dy) in &dirs {
                let (nxi, nyi) = (ix + dx, iy + dy);
                if nxi < 0 || nyi < 0 || nxi >= nx as i64 || nyi >= ny as i64 {
                    continue;
                }
                let nci = nyi as usize * nx + nxi as usize;
                for k2 in 0..self.columns[nci].len() {
                    if (self.columns[nci][k2] - z).abs() <= STEP && dist[nci][k2] > d + 1 {
                        dist[nci][k2] = d + 1;
                        q.push_back((nci, k2));
                    }
                }
            }
        }

        for ci in 0..nx * ny {
            let kept: Vec<f32> = self.columns[ci]
                .iter()
                .enumerate()
                .filter(|(k, _)| dist[ci][*k] >= radius_cells)
                .map(|(_, &z)| z)
                .collect();
            self.columns[ci] = kept;
        }
    }

    /// Find clean one-way **drops**: `(edge_origin, landing_origin)` pairs where a span sits
    /// more than `STEP` above a 4-neighbour span (a ledge the bot can walk off but not climb)
    /// and the column between is clear so it lands on the lower floor. Run on the FULL (un-
    /// eroded) heightfield so ledge edges aren't already removed; the navmesh wires each to its
    /// nearest eroded rects. `edge`/`landing` are origin positions (floor + 24).
    pub fn find_drops(&self, cm: &CollisionModel) -> Vec<([f32; 3], [f32; 3])> {
        const MAX_FALL: f32 = 400.0;
        let zero = [0.0f32; 3];
        let nx = self.nx;
        let ny = self.ny;
        let mut out = Vec::new();
        for ci in 0..nx * ny {
            let ix = (ci % nx) as i64;
            let iy = (ci / nx) as i64;
            for &z in &self.columns[ci] {
                for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                    let (nxi, nyi) = (ix + dx, iy + dy);
                    if nxi < 0 || nyi < 0 || nxi >= nx as i64 || nyi >= ny as i64 {
                        continue;
                    }
                    let nci = nyi as usize * nx + nxi as usize;
                    for &nz in &self.columns[nci] {
                        let drop = z - nz;
                        if drop <= STEP + 1.0 || drop > MAX_FALL {
                            continue;
                        }
                        let top = self.cell_center(nxi as usize, nyi as usize, z);
                        let bot = [top[0], top[1], nz - 24.0];
                        let t = cm.trace(&top, &bot, &zero, &zero, MASK_SOLID);
                        if !t.startsolid && (t.endpos[2] - (nz - 24.0)).abs() < STEP {
                            out.push((
                                self.cell_center(ix as usize, iy as usize, z),
                                self.cell_center(nxi as usize, nyi as usize, nz),
                            ));
                        }
                    }
                }
            }
        }
        out
    }

    /// Build the heightfield by probing every grid column over `bounds` (model-0 mins/maxs).
    /// Columns are independent → rasterized in parallel.
    pub fn build(cm: &CollisionModel, bounds: ([f32; 3], [f32; 3]), params: VoxelParams) -> Self {
        let cs = params.cell_size;
        let (mins, maxs) = bounds;
        let nx = (((maxs[0] - mins[0]) / cs).ceil() as usize).max(1);
        let ny = (((maxs[1] - mins[1]) / cs).ceil() as usize).max(1);

        let columns: Vec<Vec<f32>> = (0..nx * ny)
            .into_par_iter()
            .map(|idx| {
                let ix = idx % nx;
                let iy = idx / nx;
                let x = mins[0] + (ix as f32 + 0.5) * cs;
                let y = mins[1] + (iy as f32 + 0.5) * cs;
                column_floors(cm, x, y, bounds)
            })
            .collect();

        Self {
            cell_size: cs,
            nx,
            ny,
            min: [mins[0], mins[1]],
            columns,
        }
    }
}

/// Probe one column for every walkable floor (player-origin Z). Downward point-traces find
/// each solid floor top; a point **headroom** trace keeps only floors with the player's full
/// 56u standing height clear above (rejects crawlspaces / low ceilings) and non-liquid. No
/// horizontal hull test (that would erode doorways — see module docs). Then it steps down
/// through the solid into the next cavity and repeats, up to `MAX_FLOORS` stacked levels.
fn column_floors(cm: &CollisionModel, x: f32, y: f32, bounds: ([f32; 3], [f32; 3])) -> Vec<f32> {
    const MAX_FLOORS: usize = 8;
    // Player hull is -24..+32 (56u tall). Standing at origin = floor + 24, the head reaches
    // floor + 56. Require that column clear of solid (with a small slack off the surfaces).
    const PLAYER_HEIGHT: f32 = 56.0;
    let zero = [0.0f32; 3];
    let floor_min_z = bounds.0[2] - 200.0;
    let mut probe_z = bounds.1[2] + 200.0;
    let mut out = Vec::new();

    for _ in 0..MAX_FLOORS {
        let top = [x, y, probe_z];
        let bot = [x, y, floor_min_z];
        let down = cm.trace(&top, &bot, &zero, &zero, MASK_SOLID);
        if down.fraction >= 1.0 || down.startsolid {
            break; // no more floors in this column
        }

        let floor_z = down.endpos[2];
        let oz = floor_z + 24.0; // player origin sits 24u above the floor (hull mins.z = -24)
        let head_bot = [x, y, floor_z + 2.0];
        let head_top = [x, y, floor_z + PLAYER_HEIGHT];
        let up = cm.trace(&head_bot, &head_top, &zero, &zero, MASK_SOLID);
        let headroom = up.fraction >= 1.0 && !up.startsolid;
        // NB: no horizontal hull-fit erosion here — at any grid spacing it severs Q2 doorways
        // (a 32u door's only hull-fit point is its exact center, which no cell samples), so the
        // mesh fragments. Recast-style distance-field erosion (keep cells whose distance-to-
        // border ≥ radius, centerline inclusive) is the correct fix and is the next step; until
        // then bots can be routed into near-wall cells and wedge (hull embedded in solid).
        if headroom && cm.point_contents(&[x, y, oz]) & MASK_WATER == 0 {
            out.push(oz);
        }

        // Step down through the solid brush we just landed on to find the next cavity.
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

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_params_match_q2_constants() {
        let p = VoxelParams::default();
        assert_eq!(p.cell_size, 8.0);
        assert_eq!(p.walkable_climb, STEP); // 18
        assert_eq!(p.agent_radius, 16.0);
    }

    #[test]
    fn cell_center_is_cell_midpoint() {
        let hf = Heightfield {
            cell_size: 8.0,
            nx: 4,
            ny: 4,
            min: [-100.0, -200.0],
            columns: vec![Vec::new(); 16],
        };
        // Cell (0,0) center is half a cell in from min.
        assert_eq!(hf.cell_center(0, 0, 50.0), [-96.0, -196.0, 50.0]);
        // Cell (2,1): min + (2.5, 1.5) * 8.
        assert_eq!(hf.cell_center(2, 1, 0.0), [-80.0, -188.0, 0.0]);
    }
}
