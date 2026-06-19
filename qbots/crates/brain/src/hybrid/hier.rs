//! `hybrid-hier` — hierarchical global/local split. The navmesh answers "where do I go?"
//! (a strategic corridor over open polygons); the A* waypoint graph answers "how do I move
//! through here?" (local execution, including its jump/drop/ledge semantics).
//!
//! Each tick the bot's position is projected onto the navmesh funnel and a point
//! `LOCAL_HORIZON` units further along becomes a **sliding sub-goal** fed to A*. A* plans and
//! steers the last few hundred units; as the bot advances, the sub-goal slides forward along
//! the corridor. Near the end the sub-goal clamps to the real goal. If the navmesh has no
//! corridor, A* drives straight to the final goal (graceful degradation to pure `astar`).
//!
//! All steering/telemetry trait calls delegate to A* (the local executor), so jump links still
//! fire — the one capability the navmesh lacks.

use std::sync::Arc;

use glam::Vec3;

use world::{CollisionModel, NavGraph, NavMesh};

use crate::nav::NavGoal;
use crate::nav_mode::Navigator;
use crate::pursuit;

use super::{goal_key, goal_to_pos, Sub};

/// How far ahead along the navmesh corridor to place A*'s local sub-goal (units). Long enough
/// that A* plans a meaningful local route, short enough that it stays on the navmesh's corridor.
const LOCAL_HORIZON: f32 = 300.0;

/// Navmesh corridor + A* local execution (Plan 20).
pub struct HybridHier {
    sub: Sub,
    /// The real (final) goal position the navmesh routes toward.
    final_goal: Option<Vec3>,
}

impl HybridHier {
    pub fn new(graph: Arc<NavGraph>, mesh: Arc<NavMesh>, agent_radius: f32) -> Self {
        Self {
            sub: Sub::new(graph, mesh, agent_radius),
            final_goal: None,
        }
    }

    /// The local sub-goal: a point `LOCAL_HORIZON` along the navmesh corridor from the bot's
    /// projection, or the final goal itself when there is no corridor.
    fn subgoal(&self, from: Vec3) -> Vec3 {
        let path = self.sub.navmesh.path();
        if path.len() >= 2 {
            let (seg, t) = pursuit::project_onto_path(path, from);
            pursuit::point_ahead(path, seg, t, LOCAL_HORIZON)
        } else {
            self.final_goal.unwrap_or(from)
        }
    }
}

impl Navigator for HybridHier {
    fn set_goal(&mut self, goal: NavGoal, from: Vec3) {
        self.final_goal = Some(goal_key(&self.sub.graph, &goal));
        // Keep the navmesh corridor planned/fresh (cheap — replans only on change or loss).
        self.sub
            .navmesh
            .set_goal(goal_to_pos(&self.sub.graph, &goal), from);
        // Hand A* the sliding local sub-goal along that corridor.
        let sub_goal = self.subgoal(from);
        self.sub.astar.set_goal(NavGoal::Position(sub_goal), from);
    }

    fn update(&mut self, pos: Vec3, cm: Option<&CollisionModel>) -> bool {
        // Track the navmesh projection (for the next subgoal) and drive A*'s internal advance.
        self.sub.navmesh.update(pos, cm);
        self.sub.astar.update(pos, cm);
        // Reach is determined by the scenario/loop from position vs the real goal (mirrors the
        // navmesh backend): A* reaching a *transient* sub-goal must not be reported as "done".
        false
    }

    fn pursue_target(&self, from: Vec3) -> Option<Vec3> {
        self.sub.astar.pursue_target(from)
    }

    fn pursue_target_safe(&self, from: Vec3, cm: &CollisionModel) -> Option<Vec3> {
        self.sub.astar.pursue_target_safe(from, cm)
    }

    fn current_edge_is_jump(&self) -> bool {
        self.sub.astar.current_edge_is_jump()
    }

    fn current_edge_is_swim(&self) -> bool {
        self.sub.astar.current_edge_is_swim()
    }

    fn force_replan(&mut self) {
        // Replan both layers: a new corridor and a fresh local route off it.
        self.sub.navmesh.force_replan();
        self.sub.astar.force_replan();
    }

    fn blacklist_waypoint_if_blocked(&mut self, pos: Vec3, cm: &CollisionModel) {
        self.sub.astar.blacklist_waypoint_if_blocked(pos, cm);
    }

    fn current_waypoint(&self) -> Option<usize> {
        self.sub.astar.current_waypoint()
    }

    fn current_waypoint_pos(&self) -> Option<[f32; 3]> {
        Navigator::current_waypoint_pos(&self.sub.astar)
    }

    fn smooth_with_cm(&mut self, cm: &CollisionModel, from: Vec3) {
        self.sub.astar.smooth_with_cm(cm, from);
    }

    fn set_risk_overlay(&mut self, overlay: Vec<f32>) {
        self.sub.astar.set_risk_overlay(overlay);
    }

    fn goal_abandoned(&self) -> bool {
        self.sub.astar.goal_abandoned()
    }

    fn speed_scale(&self, pos: Vec3) -> f32 {
        self.sub.astar.speed_scale(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_corridor_falls_back_to_astar_toward_final_goal() {
        let g = Arc::new(NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            vec![vec![(1, 100.0)], vec![(0, 100.0)]],
        ));
        // Empty navmesh → no corridor; A* must still drive toward the final goal node.
        let mut d = HybridHier::new(g, Arc::new(NavMesh::empty()), 16.0);
        d.set_goal(NavGoal::Waypoint(1), Vec3::ZERO);
        assert_eq!(
            d.current_waypoint(),
            Some(1),
            "A* plans straight to the goal when there is no navmesh corridor"
        );
    }
}
