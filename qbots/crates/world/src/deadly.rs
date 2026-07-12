//! Deadly-floor (lava/slime) validation primitives — shared by BOTH nav builders.
//!
//! `MASK_SOLID` traces pass straight through liquids and stop on the solid *bed*
//! beneath a pool, so any floor probe that only traces solids will happily call a
//! lava bed "walkable floor". These pure functions are the counter-checks: they
//! probe the *surface* contents above a floor hit and the momentum-overshoot strip
//! past a landing. Shipped for the A* graph in Plans 48/50 (cache v21–v23); moved
//! here in Plan 63 because they were private to `navgraph` — the concrete reason
//! the navmesh builder shipped without them and routed bots into q2dm6 lava.
//!
//! Callers: `navgraph` (node sampling, walk edges, stair treads, jump/drop
//! landings), `navmesh::heightfield` (span acceptance, drop landings), and
//! `brain`'s steer validation (`pursue_target_safe` / `steer_line_safe`).

use crate::collision::{CollisionModel, CONTENTS_LAVA, CONTENTS_SLIME, MASK_SOLID};

/// True when the solid floor at `endpos` (a down-trace hit point) lies under lava or
/// slime — standing there is death, so callers must not treat it as walkable support.
pub fn floor_is_deadly(cm: &CollisionModel, endpos: &[f32; 3]) -> bool {
    let above = [endpos[0], endpos[1], endpos[2] + 1.0];
    cm.point_contents(&above) & (CONTENTS_LAVA | CONTENTS_SLIME) != 0
}

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

/// True if a jump/fall LANDING at `base` (foot/origin level) with horizontal travel
/// direction `dir` touches lava/slime anywhere on the 0–48 u overshoot strip (Plan 50 E3).
/// A bot arrives with momentum under 10 Hz control — it does not stop dead on the landing
/// point; if the strip it skids across hangs over a lava channel, the edge is a death trap.
/// Every soak-verified q2dm3 lava entry was a FALL (vz −240..−690) clustered on such
/// landings.
pub fn landing_strip_deadly(cm: &CollisionModel, base: [f32; 3], dir: [f32; 2]) -> bool {
    // A skid that leaves the strip FALLS — and a fall that ends in lava is death at ANY
    // depth (q2dm6 telemetry: entries 100–280u below landings only 13–30u away). Probe to
    // past MAX_FALL instead of a step-down horizon (was 72, then 96, pre-/early-Plan-63).
    const FALL_PROBE: f32 = 512.0;
    let zero = [0.0f32; 3];
    // Bots arrive with momentum in the PATH direction, not the drop's axis — they skid
    // sideways off the landing too (live entries 22u to the SIDE of validated landings).
    // Sample the drop direction plus both perpendiculars.
    let perp = [-dir[1], dir[0]];
    for ray in [dir, perp, [-perp[0], -perp[1]]] {
        for d in [0.0f32, 16.0, 32.0, 48.0] {
            let p = [base[0] + ray[0] * d, base[1] + ray[1] * d, base[2] + 8.0];
            if cm.point_contents(&p) & (CONTENTS_LAVA | CONTENTS_SLIME) != 0 {
                return true;
            }
            let down = [p[0], p[1], p[2] - FALL_PROBE];
            let t = cm.trace(&p, &down, &zero, &zero, MASK_SOLID);
            if !t.startsolid && t.fraction < 1.0 && floor_is_deadly(cm, &t.endpos) {
                return true;
            }
        }
    }
    false
}
