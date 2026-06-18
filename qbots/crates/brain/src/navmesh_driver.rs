//! The navmesh backend's [`Navigator`] — drives a bot along a funnel path over a [`NavMesh`].
//!
//! Mirrors the role of [`crate::nav::NavigationDriver`] but over polygons instead of a
//! waypoint graph. Progress is **projection-native** by construction: the bot's arc-length
//! along the funnel polyline is the only progress signal, so there is none of the
//! reach/orbit/give-up density coupling that made the waypoint driver grid-sensitive. Wall
//! clearance comes from the funnel's portal inset; `pursue_target_safe` hull-validates the
//! aim point as a backstop.

use std::sync::Arc;

use glam::Vec3;

use world::collision::MASK_SOLID;
use world::navgraph::{segment_has_floor, HULL_MAXS, HULL_MINS};
use world::{CollisionModel, NavMesh};

use crate::nav::NavGoal;
use crate::nav_mode::Navigator;
use crate::pursuit;

/// Fixed pure-pursuit look-ahead (units) — a steering-smoothness constant, not grid-scaled.
const LOOKAHEAD: f32 = 96.0;
/// Goal moved more than this (units) → replan.
const GOAL_MOVED: f32 = 16.0;
/// Ticks to wait before retrying a replan that produced no path (avoids per-tick A*).
const REPLAN_COOLDOWN: i32 = 5;

/// Drives a bot along navmesh funnel paths.
pub struct NavmeshDriver {
    mesh: Arc<NavMesh>,
    radius: f32,
    /// Current funnel polyline (`start … goal`); empty when there is no plan.
    path: Vec<Vec3>,
    goal: Option<Vec3>,
    /// Projection segment from the last `update` (steering + telemetry).
    seg: usize,
    cooldown: i32,
}

impl NavmeshDriver {
    pub fn new(mesh: Arc<NavMesh>, agent_radius: f32) -> Self {
        Self {
            mesh,
            radius: agent_radius,
            path: Vec::new(),
            goal: None,
            seg: 0,
            cooldown: 0,
        }
    }

    fn replan(&mut self, from: Vec3, goal: Vec3) {
        let a = [from.x, from.y, from.z];
        let g = [goal.x, goal.y, goal.z];
        self.path = match self.mesh.path(a, g, self.radius) {
            Some(p) => p.into_iter().map(Vec3::from).collect(),
            None => Vec::new(),
        };
        self.seg = 0;
        self.cooldown = REPLAN_COOLDOWN;
    }

    /// Aim point along the path, or `None` if there is no usable plan.
    fn aim(&self, from: Vec3) -> Option<(usize, f32, Vec3)> {
        if self.path.len() < 2 {
            return None;
        }
        let (seg, t) = pursuit::project_onto_path(&self.path, from);
        Some((seg, t, pursuit::point_ahead(&self.path, seg, t, LOOKAHEAD)))
    }
}

impl Navigator for NavmeshDriver {
    fn set_goal(&mut self, goal: NavGoal, from: Vec3) {
        let g = match goal {
            NavGoal::Position(p) | NavGoal::Entity(p) => p,
            // The navmesh has no waypoint indices; ignore index goals.
            NavGoal::Waypoint(_) => return,
        };
        let changed = self
            .goal
            .map(|og| (og - g).length() > GOAL_MOVED)
            .unwrap_or(true);
        if changed {
            self.goal = Some(g);
            self.replan(from, g);
        } else if self.path.len() < 2 {
            // Lost the path (stall/blacklist cleared it) — retry on a cooldown.
            if self.cooldown <= 0 {
                self.replan(from, g);
            } else {
                self.cooldown -= 1;
            }
        }
    }

    fn update(&mut self, pos: Vec3, _cm: Option<&CollisionModel>) -> bool {
        // Only track the projection segment (for steering + telemetry). We do NOT abandon the
        // path on slow path-progress here: a bot legitimately slows to turn a corner, which
        // would false-trigger a clear and make it STOP in the open (pause/hang → pile-ups). Real
        // stalls are caught by the scenario's position-based StuckDetector, which calls
        // force_replan/blacklist_waypoint_if_blocked.
        if self.path.len() >= 2 {
            let (seg, _t) = pursuit::project_onto_path(&self.path, pos);
            self.seg = seg;
        }
        false
    }

    fn pursue_target(&self, from: Vec3) -> Option<Vec3> {
        self.aim(from).map(|(_, _, p)| p)
    }

    fn pursue_target_safe(&self, from: Vec3, cm: &CollisionModel) -> Option<Vec3> {
        let (seg, _t, raw) = self.aim(from)?;
        let a = [from.x, from.y, from.z];
        let b = [raw.x, raw.y, raw.z];
        let tr = cm.trace(&a, &b, &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if !(tr.startsolid || tr.fraction < 1.0) && segment_has_floor(cm, a, b) {
            return Some(raw);
        }
        // Unsafe straight line — fall back to the next path vertex forward of the projection
        // (always on the walkable funnel polyline).
        let nxt = (seg + 1).min(self.path.len() - 1);
        Some(self.path[nxt])
    }

    fn current_edge_is_jump(&self) -> bool {
        false // off-mesh jump links are Phase 5
    }

    fn force_replan(&mut self) {
        self.path.clear();
        self.cooldown = 0;
    }

    fn blacklist_waypoint_if_blocked(&mut self, _pos: Vec3, _cm: &CollisionModel) {
        // No poly blacklist yet; just drop the plan so set_goal replans.
        self.path.clear();
        self.cooldown = 0;
    }

    fn current_waypoint(&self) -> Option<usize> {
        (self.path.len() >= 2).then_some(self.seg)
    }

    fn current_waypoint_pos(&self) -> Option<[f32; 3]> {
        if self.path.len() < 2 {
            return None;
        }
        let nxt = (self.seg + 1).min(self.path.len() - 1);
        let v = self.path[nxt];
        Some([v.x, v.y, v.z])
    }

    fn goal_abandoned(&self) -> bool {
        false // navmesh never self-abandons a goal; recovery handles real stalls
    }

    /// Tunable global speed (default 1.0). A cut reduces tight-doorway overshoot but slows long
    /// routes into timeouts (0.7 regressed both s2s and RL), so it's left at full speed.
    fn speed_scale(&self, _pos: Vec3) -> f32 {
        std::env::var("QBOTS_SPEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0)
    }
}
