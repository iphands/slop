//! `hybrid-fallback` — A* drives by default; on a hard-stuck it hands the current segment to
//! the navmesh, and returns to A* on the next goal.
//!
//! The hard-stuck signal is external: the tick loop runs a position-based `StuckDetector` and
//! calls [`Navigator::force_replan`] when the bot has made no progress. This backend interprets
//! a `force_replan` *while A* is active* as "A* is wedged here" and switches the active driver
//! to the navmesh (which routes over open polygons and ignores the graph's false edges). A
//! changed goal re-arms A* — every fresh objective gets the graph's richer traversal first.

use std::sync::Arc;

use glam::Vec3;

use world::{CollisionModel, NavGraph, NavMesh};

use crate::nav::NavGoal;
use crate::nav_mode::Navigator;

use super::{goal_key, goal_to_pos, Backend, Sub, GOAL_MOVED};

/// A* primary, navmesh on stuck (Plan 20).
pub struct HybridFallback {
    sub: Sub,
    active: Backend,
    /// Resolved position of the goal we last planned for; drives the "new goal" re-arm.
    last_goal: Option<Vec3>,
}

impl HybridFallback {
    pub fn new(graph: Arc<NavGraph>, mesh: Arc<NavMesh>, agent_radius: f32) -> Self {
        Self {
            sub: Sub::new(graph, mesh, agent_radius),
            active: Backend::Astar,
            last_goal: None,
        }
    }
}

impl Navigator for HybridFallback {
    fn set_goal(&mut self, goal: NavGoal, from: Vec3) {
        let key = goal_key(&self.sub.graph, &goal);
        let changed = self
            .last_goal
            .is_none_or(|k| (k - key).length() > GOAL_MOVED);
        if changed {
            // A fresh objective: give the graph the first shot again.
            self.active = Backend::Astar;
            self.last_goal = Some(key);
        }
        match self.active {
            Backend::Astar => self.sub.astar.set_goal(goal, from),
            Backend::Navmesh => self
                .sub
                .navmesh
                .set_goal(goal_to_pos(&self.sub.graph, &goal), from),
        }
    }

    fn update(&mut self, pos: Vec3, cm: Option<&CollisionModel>) -> bool {
        match self.active {
            Backend::Astar => self.sub.astar.update(pos, cm),
            Backend::Navmesh => self.sub.navmesh.update(pos, cm),
        }
    }

    fn pursue_target(&self, from: Vec3) -> Option<Vec3> {
        match self.active {
            Backend::Astar => self.sub.astar.pursue_target(from),
            Backend::Navmesh => self.sub.navmesh.pursue_target(from),
        }
    }

    fn pursue_target_safe(&self, from: Vec3, cm: &CollisionModel) -> Option<Vec3> {
        match self.active {
            Backend::Astar => self.sub.astar.pursue_target_safe(from, cm),
            Backend::Navmesh => self.sub.navmesh.pursue_target_safe(from, cm),
        }
    }

    fn current_edge_is_jump(&self) -> bool {
        match self.active {
            Backend::Astar => self.sub.astar.current_edge_is_jump(),
            Backend::Navmesh => false,
        }
    }

    fn current_edge_is_swim(&self) -> bool {
        match self.active {
            Backend::Astar => self.sub.astar.current_edge_is_swim(),
            Backend::Navmesh => false,
        }
    }

    fn force_replan(&mut self) {
        match self.active {
            // A* is wedged — switch to the navmesh for the rest of this goal. Clearing the
            // navmesh path (force_replan resets its cooldown) makes the next `set_goal` plan it.
            Backend::Astar => {
                self.active = Backend::Navmesh;
                self.sub.navmesh.force_replan();
            }
            Backend::Navmesh => self.sub.navmesh.force_replan(),
        }
    }

    fn blacklist_waypoint_if_blocked(&mut self, pos: Vec3, cm: &CollisionModel) {
        match self.active {
            Backend::Astar => self.sub.astar.blacklist_waypoint_if_blocked(pos, cm),
            Backend::Navmesh => self.sub.navmesh.blacklist_waypoint_if_blocked(pos, cm),
        }
    }

    fn current_waypoint(&self) -> Option<usize> {
        match self.active {
            Backend::Astar => self.sub.astar.current_waypoint(),
            Backend::Navmesh => Navigator::current_waypoint(&self.sub.navmesh),
        }
    }

    fn current_waypoint_pos(&self) -> Option<[f32; 3]> {
        match self.active {
            Backend::Astar => Navigator::current_waypoint_pos(&self.sub.astar),
            Backend::Navmesh => Navigator::current_waypoint_pos(&self.sub.navmesh),
        }
    }

    fn smooth_with_cm(&mut self, cm: &CollisionModel, from: Vec3) {
        // Only A* has a node path to string-pull; navmesh paths are already smooth.
        if self.active == Backend::Astar {
            self.sub.astar.smooth_with_cm(cm, from);
        }
    }

    fn set_risk_overlay(&mut self, overlay: Vec<f32>) {
        // The overlay is graph-node-indexed; always feed it to A* so it's warm on switch-back.
        self.sub.astar.set_risk_overlay(overlay);
    }

    fn goal_abandoned(&self) -> bool {
        // Only A* abandons goals (give-up watchdog); report it only while A* drives.
        self.active == Backend::Astar && self.sub.astar.goal_abandoned()
    }

    fn speed_scale(&self, pos: Vec3) -> f32 {
        match self.active {
            Backend::Astar => self.sub.astar.speed_scale(pos),
            Backend::Navmesh => self.sub.navmesh.speed_scale(pos),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn driver() -> HybridFallback {
        let g = Arc::new(NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            vec![vec![(1, 100.0)], vec![(0, 100.0)]],
        ));
        HybridFallback::new(g, Arc::new(NavMesh::empty()), 16.0)
    }

    #[test]
    fn force_replan_switches_to_navmesh_then_new_goal_rearms_astar() {
        let mut d = driver();
        d.set_goal(NavGoal::Waypoint(1), Vec3::ZERO);
        assert_eq!(d.active, Backend::Astar, "fresh goal starts on A*");

        d.force_replan();
        assert_eq!(
            d.active,
            Backend::Navmesh,
            "hard-stuck hands off to navmesh"
        );

        // Re-issuing the SAME goal keeps the navmesh in control.
        d.set_goal(NavGoal::Waypoint(1), Vec3::ZERO);
        assert_eq!(d.active, Backend::Navmesh, "same goal stays on navmesh");

        // A new (far) goal re-arms A* — every fresh objective gets the graph first.
        d.set_goal(NavGoal::Position(Vec3::new(500.0, 0.0, 0.0)), Vec3::ZERO);
        assert_eq!(d.active, Backend::Astar, "new goal returns to A*");
    }
}
