//! `hybrid-race` — plan both backends for each new goal, score them, and run the
//! cheaper-scoring one to completion (a supervisor that picks per goal, not per tick).
//!
//! Scoring is "effective cost": raw path length plus a jump-link penalty (jumps are riskier
//! than walking, so A* paths that lean on them are nudged back) plus a per-backend recent-stuck
//! bias (a backend that just wedged here is temporarily less attractive). The winner drives
//! until the goal changes; stuck recovery replans the *active* backend rather than switching —
//! that reactive switch is `hybrid-fallback`'s job, and keeping them distinct makes the A/B
//! comparison meaningful.

use std::sync::Arc;

use glam::Vec3;

use world::{CollisionModel, NavGraph, NavMesh};

use crate::nav::NavGoal;
use crate::nav_mode::Navigator;

use super::{goal_key, goal_to_pos, Backend, Sub, GOAL_MOVED};

/// Effective-cost units added per jump link on an A* path (jumps are riskier than walking).
const JUMP_PENALTY: f32 = 64.0;
/// Effective-cost units added per recent-stuck event charged to a backend (fades each new goal).
const STUCK_BIAS: f32 = 256.0;

/// Pick the lower-scoring backend. `None` means that backend produced no usable plan; if both
/// planned, the cheaper wins; if only one planned, it wins; if neither did, default to A*
/// (its recovery is richer, so it is the safer fallback when both are out of ideas).
fn pick_backend(a_score: Option<f32>, m_score: Option<f32>) -> Backend {
    match (a_score, m_score) {
        (Some(a), Some(m)) => {
            if m < a {
                Backend::Navmesh
            } else {
                Backend::Astar
            }
        }
        (Some(_), None) => Backend::Astar,
        (None, Some(_)) => Backend::Navmesh,
        (None, None) => Backend::Astar,
    }
}

/// Plan both, run the winner (Plan 20).
pub struct HybridRace {
    sub: Sub,
    active: Backend,
    last_goal: Option<Vec3>,
    /// Recent-stuck bias per backend `[astar, navmesh]`; bumped on `force_replan`, halved each
    /// new goal so an old wedge fades instead of permanently condemning a backend.
    recent_stuck: [f32; 2],
}

impl HybridRace {
    pub fn new(graph: Arc<NavGraph>, mesh: Arc<NavMesh>, agent_radius: f32) -> Self {
        Self {
            sub: Sub::new(graph, mesh, agent_radius),
            active: Backend::Astar,
            last_goal: None,
            recent_stuck: [0.0, 0.0],
        }
    }

    fn bias(&self, b: Backend) -> f32 {
        match b {
            Backend::Astar => self.recent_stuck[0],
            Backend::Navmesh => self.recent_stuck[1],
        }
    }

    fn bump_stuck(&mut self, b: Backend) {
        match b {
            Backend::Astar => self.recent_stuck[0] += 1.0,
            Backend::Navmesh => self.recent_stuck[1] += 1.0,
        }
    }
}

impl Navigator for HybridRace {
    fn set_goal(&mut self, goal: NavGoal, from: Vec3) {
        let key = goal_key(&self.sub.graph, &goal);
        let changed = self
            .last_goal
            .is_none_or(|k| (k - key).length() > GOAL_MOVED);
        if !changed {
            // Same goal — keep driving the winner; let it refresh its own plan if exhausted.
            match self.active {
                Backend::Astar => self.sub.astar.set_goal(goal, from),
                Backend::Navmesh => self
                    .sub
                    .navmesh
                    .set_goal(goal_to_pos(&self.sub.graph, &goal), from),
            }
            return;
        }
        self.last_goal = Some(key);
        self.recent_stuck[0] *= 0.5;
        self.recent_stuck[1] *= 0.5;

        // Plan with both, then score.
        self.sub.astar.set_goal(goal.clone(), from);
        self.sub
            .navmesh
            .set_goal(goal_to_pos(&self.sub.graph, &goal), from);
        let a_score = self.sub.astar.planned_cost().map(|c| {
            c + JUMP_PENALTY * self.sub.astar.planned_jump_count() as f32
                + STUCK_BIAS * self.bias(Backend::Astar)
        });
        let m_score = self
            .sub
            .navmesh
            .planned_len()
            .map(|l| l + STUCK_BIAS * self.bias(Backend::Navmesh));
        self.active = pick_backend(a_score, m_score);
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
        // The active backend wedged here — bias the next race against it, then replan it.
        self.bump_stuck(self.active);
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
    fn pick_backend_prefers_lower_score_and_handles_missing_plans() {
        assert_eq!(pick_backend(Some(100.0), Some(80.0)), Backend::Navmesh);
        assert_eq!(pick_backend(Some(80.0), Some(100.0)), Backend::Astar);
        // Ties go to A* (richer recovery).
        assert_eq!(pick_backend(Some(50.0), Some(50.0)), Backend::Astar);
        // Only one backend planned → it wins.
        assert_eq!(pick_backend(Some(999.0), None), Backend::Astar);
        assert_eq!(pick_backend(None, Some(10.0)), Backend::Navmesh);
        // Neither planned → default to A*.
        assert_eq!(pick_backend(None, None), Backend::Astar);
    }

    #[test]
    fn race_with_no_navmesh_runs_astar() {
        let g = Arc::new(NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            vec![vec![(1, 100.0)], vec![(0, 100.0)]],
        ));
        let mut d = HybridRace::new(g, Arc::new(NavMesh::empty()), 16.0);
        d.set_goal(NavGoal::Waypoint(1), Vec3::ZERO);
        // The empty navmesh produces no plan, so A* (the only planner) wins.
        assert_eq!(d.active, Backend::Astar);
    }
}
