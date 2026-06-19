//! `hybrid-segment` — segment ownership. The navmesh owns open-space routing; the A* graph
//! owns **jump-link segments** — the one traversal the navmesh cannot do (its
//! `current_edge_is_jump` is always false, so a pure-navmesh bot can never press jump).
//!
//! Default driver is the navmesh. Each tick, if the bot is near a graph node that has an
//! outgoing jump edge pointed roughly toward the goal, the graph takes over so it can execute
//! the launch (face `launch_yaw`, press jump+forward). Once the jump link is behind the bot,
//! control returns to the navmesh for the next open stretch.

use std::sync::Arc;

use glam::Vec3;

use world::{CollisionModel, EdgeKind, NavGraph, NavMesh};

use crate::nav::NavGoal;
use crate::nav_mode::Navigator;

use super::{goal_key, goal_to_pos, Backend, Sub};

/// Hand off to A* when the bot is within this distance (units) of a node with a goal-ward jump.
const TRIGGER_DIST: f32 = 96.0;
/// A jump edge counts as "toward the goal" when its direction's cosine with the goal direction
/// exceeds this (≈ within 75° of straight-at-goal) — generous, since jump links are sparse.
const GOALWARD_COS: f32 = 0.26;

/// Navmesh open routing + A* jump-link segments (Plan 20).
pub struct HybridSegment {
    sub: Sub,
    active: Backend,
    final_goal: Option<Vec3>,
}

impl HybridSegment {
    pub fn new(graph: Arc<NavGraph>, mesh: Arc<NavMesh>, agent_radius: f32) -> Self {
        Self {
            sub: Sub::new(graph, mesh, agent_radius),
            active: Backend::Navmesh,
            final_goal: None,
        }
    }

    /// True if the bot is near a graph node with an outgoing jump edge aimed at the goal — the
    /// cue to hand the segment to A* so it can execute the launch.
    fn jump_link_ahead(&self, from: Vec3) -> bool {
        let Some(goal) = self.final_goal else {
            return false;
        };
        let to_goal = goal - from;
        if to_goal.length_squared() < 1.0 {
            return false;
        }
        let to_goal = to_goal.normalize();
        let g = &self.sub.graph;
        let Some(n) = g.nearest(&[from.x, from.y, from.z]) else {
            return false;
        };
        if (Vec3::from(g.node_pos(n)) - from).length() > TRIGGER_DIST {
            return false; // not actually at this node yet
        }
        g.neighbors(n).iter().any(|&(to, _)| {
            if !matches!(g.edge_kind(n, to), EdgeKind::Jump { .. }) {
                return false;
            }
            let jdir = Vec3::from(g.node_pos(to)) - Vec3::from(g.node_pos(n));
            jdir.length_squared() > 1.0 && jdir.normalize().dot(to_goal) > GOALWARD_COS
        })
    }
}

impl Navigator for HybridSegment {
    fn set_goal(&mut self, goal: NavGoal, from: Vec3) {
        self.final_goal = Some(goal_key(&self.sub.graph, &goal));
        let near_jump = self.jump_link_ahead(from);
        self.active = match self.active {
            // Open space — switch to A* only when a goal-ward jump link is right here.
            Backend::Navmesh if near_jump => Backend::Astar,
            // Executing a jump — stay on A* until the launch is done and no new jump is pending.
            Backend::Astar if self.sub.astar.current_edge_is_jump() || near_jump => Backend::Astar,
            Backend::Astar => Backend::Navmesh,
            other => other,
        };
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

    fn force_replan(&mut self) {
        match self.active {
            Backend::Astar => self.sub.astar.force_replan(),
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
        if self.active == Backend::Astar {
            self.sub.astar.smooth_with_cm(cm, from);
        }
    }

    fn set_risk_overlay(&mut self, overlay: Vec<f32>) {
        self.sub.astar.set_risk_overlay(overlay);
    }

    fn goal_abandoned(&self) -> bool {
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

    #[test]
    fn switches_to_astar_when_a_goalward_jump_link_is_underfoot() {
        // A(0)→B(1) is a jump link pointing +x; the goal is further along +x.
        let g = Arc::new(NavGraph::from_raw_with_jumps(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, -40.0], [200.0, 0.0, -40.0]],
            vec![vec![(1, 100.0)], vec![(2, 100.0)], vec![]],
            vec![(0, 1, 0.0)],
        ));
        let mut d = HybridSegment::new(g, Arc::new(NavMesh::empty()), 16.0);
        // Standing on node A, goal at node C (+x) → the A→B jump is goal-ward → A* takes over.
        d.set_goal(NavGoal::Waypoint(2), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(
            d.active,
            Backend::Astar,
            "goal-ward jump link hands off to A*"
        );
        // A* now plans through the jump edge (its current_edge_is_jump fires once it advances
        // off the start node and prev_waypoint is set — first-edge timing is a nav-driver detail).
        assert_eq!(
            d.current_waypoint(),
            Some(1),
            "A* heads for the jump landing node"
        );
    }

    #[test]
    fn stays_on_navmesh_in_open_space() {
        // No jump edges at all → never hands off to A*.
        let g = Arc::new(NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            vec![vec![(1, 100.0)], vec![(0, 100.0)]],
        ));
        let mut d = HybridSegment::new(g, Arc::new(NavMesh::empty()), 16.0);
        d.set_goal(NavGoal::Waypoint(1), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(d.active, Backend::Navmesh);
    }
}
