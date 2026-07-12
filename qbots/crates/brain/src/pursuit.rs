//! Pure-pursuit over a 3D polyline — the density-independent steering primitive shared by
//! the navigation backends. Given the bot's position and a path polyline, project onto it
//! and aim a fixed look-ahead distance further along. The navmesh funnel path is already a
//! `Vec<Vec3>` polyline, so these operate on it directly.

use glam::Vec3;

/// Project `from` onto the polyline; return `(segment_index, t∈[0,1])` of the closest point.
/// Searches all segments (funnel paths are short), so it recovers even if the bot is pushed
/// off the path. Returns `(0, 0.0)` for a degenerate path.
pub fn project_onto_path(path: &[Vec3], from: Vec3) -> (usize, f32) {
    let mut best = (0usize, 0.0f32, f32::INFINITY);
    if path.len() < 2 {
        return (0, 0.0);
    }
    for seg in 0..path.len() - 1 {
        let a = path[seg];
        let ab = path[seg + 1] - a;
        let len2 = ab.length_squared();
        let t = if len2 < 1e-6 {
            0.0
        } else {
            ((from - a).dot(ab) / len2).clamp(0.0, 1.0)
        };
        let d = (from - (a + ab * t)).length_squared();
        if d < best.2 {
            best = (seg, t, d);
        }
    }
    (best.0, best.1)
}

/// The point `dist` units ahead of `(seg, t)` along the polyline (clamped to the final
/// vertex). This is the pure-pursuit aim point.
pub fn point_ahead(path: &[Vec3], seg: usize, t: f32, dist: f32) -> Vec3 {
    if path.len() < 2 {
        return path.first().copied().unwrap_or(Vec3::ZERO);
    }
    let mut cur = seg.min(path.len() - 2);
    let mut remaining = dist;
    // Account for the partial first segment from t.
    let mut start_t = t;
    loop {
        let a = path[cur];
        let b = path[cur + 1];
        let seg_vec = b - a;
        let seg_len = seg_vec.length().max(1e-6);
        let rem_on_seg = seg_len * (1.0 - start_t);
        if remaining <= rem_on_seg || cur + 2 >= path.len() {
            let tt = (start_t + remaining / seg_len).min(1.0);
            return a + seg_vec * tt;
        }
        remaining -= rem_on_seg;
        cur += 1;
        start_t = 0.0;
    }
}

/// True when steering straight from `from` to `to` is safe: hull-clear (no wall clip) and
/// floor-continuous / non-deadly (no gap or lava under the line). The single shared
/// validation both `pursue_target_safe` impls use (Plan 63 — extracted from the identical
/// checks the A* and navmesh drivers each hand-rolled); the *fallback policy* when it fails
/// stays per-driver (A*'s graph node is safe by construction, the navmesh's funnel vertex
/// must be re-validated).
pub fn steer_line_safe(cm: &world::CollisionModel, from: Vec3, to: Vec3) -> bool {
    use world::navgraph::{segment_has_floor, HULL_MAXS, HULL_MINS};
    let a = [from.x, from.y, from.z];
    let b = [to.x, to.y, to.z];
    let t = cm.trace(&a, &b, &HULL_MINS, &HULL_MAXS, world::MASK_SOLID);
    !(t.startsolid || t.fraction < 1.0) && segment_has_floor(cm, a, b)
}

/// Arc-length from the path start to `(seg, t)` — the bot's forward progress, independent of
/// how the path is vertexed.
pub fn arc_length(path: &[Vec3], seg: usize, t: f32) -> f32 {
    if path.len() < 2 {
        return 0.0;
    }
    let seg = seg.min(path.len() - 2);
    let mut acc = 0.0;
    for s in 0..seg {
        acc += (path[s + 1] - path[s]).length();
    }
    acc + (path[seg + 1] - path[seg]).length() * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steer_line_safe_passes_on_flat_floor() {
        // Floor just under the origin — hull-clear and continuous floor.
        let cm = world::CollisionModel::half_space([0.0, 0.0, 1.0], -0.25);
        assert!(steer_line_safe(
            &cm,
            Vec3::new(0.0, 0.0, 24.0),
            Vec3::new(96.0, 0.0, 24.0)
        ));
    }

    #[test]
    fn steer_line_safe_rejects_gap_and_wall() {
        // Bottomless world: the hull line is clear but there is no floor under it.
        let gap = world::CollisionModel::half_space([0.0, 0.0, 1.0], -100_000.0);
        assert!(!steer_line_safe(
            &gap,
            Vec3::new(0.0, 0.0, 24.0),
            Vec3::new(96.0, 0.0, 24.0)
        ));
        // Wall at x=0 (solid x<0): a line crossing it is hull-blocked.
        let wall = world::CollisionModel::half_space([1.0, 0.0, 0.0], 0.0);
        assert!(!steer_line_safe(
            &wall,
            Vec3::new(50.0, 0.0, 24.0),
            Vec3::new(-50.0, 0.0, 24.0)
        ));
    }

    fn line() -> Vec<Vec3> {
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(100.0, 0.0, 0.0),
            Vec3::new(200.0, 0.0, 0.0),
        ]
    }

    #[test]
    fn project_finds_nearest_segment_point() {
        let p = line();
        let (seg, t) = project_onto_path(&p, Vec3::new(150.0, 10.0, 0.0));
        assert_eq!(seg, 1);
        assert!((t - 0.5).abs() < 1e-3);
    }

    #[test]
    fn point_ahead_walks_across_segments() {
        let p = line();
        // 60 ahead of x=50 → x=110.
        let ahead = point_ahead(&p, 0, 0.5, 60.0);
        assert!((ahead.x - 110.0).abs() < 1e-2, "{ahead:?}");
    }

    #[test]
    fn point_ahead_clamps_to_end() {
        let p = line();
        let ahead = point_ahead(&p, 1, 0.5, 9999.0);
        assert!((ahead.x - 200.0).abs() < 1e-2);
    }

    #[test]
    fn arc_length_accumulates() {
        let p = line();
        assert!((arc_length(&p, 1, 0.5) - 150.0).abs() < 1e-2);
    }
}
