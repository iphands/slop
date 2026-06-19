//! Hybrid navigation backends (Plan 20) — thin [`Navigator`](crate::nav_mode::Navigator)
//! supervisors that delegate between the A* waypoint-graph driver
//! ([`NavigationDriver`](crate::nav::NavigationDriver)) and the navmesh driver
//! ([`NavmeshDriver`](crate::navmesh_driver::NavmeshDriver)).
//!
//! Each mode owns **both** sub-drivers and decides, per trait call, which one answers. The
//! heavy lifting (planning, stuck recovery, jump links) stays in the proven sub-drivers; the
//! hybrids only choose *which* backend drives at any moment. They are selected by
//! `--mode hybrid-{fallback,race,hier,segment}`.
//!
//! - [`fallback::HybridFallback`] — A* primary; hand the segment to navmesh on a hard-stuck.
//! - [`race::HybridRace`] — plan both per goal, run the cheaper-scoring backend to completion.
//! - [`hier::HybridHier`] — navmesh picks the corridor, A* executes a sliding local sub-goal.
//! - [`segment::HybridSegment`] — navmesh routes open space, A* owns jump-link segments.

use std::sync::Arc;

use glam::Vec3;

use world::{NavGraph, NavMesh};

use crate::nav::{NavGoal, NavigationDriver};
use crate::navmesh_driver::NavmeshDriver;

pub mod fallback;

pub use fallback::HybridFallback;

/// A goal that moves less than this (units) between ticks is treated as unchanged — matches
/// the navmesh driver's own replan threshold, so the hybrids and the navmesh agree on "new goal".
pub(crate) const GOAL_MOVED: f32 = 16.0;

/// Which sub-driver is currently driving the bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backend {
    Astar,
    Navmesh,
}

/// The two sub-drivers a hybrid owns, plus the shared graph (needed to translate waypoint
/// goals into world positions for the navmesh and to inspect edges for jump-link routing).
pub(crate) struct Sub {
    pub graph: Arc<NavGraph>,
    pub astar: NavigationDriver,
    pub navmesh: NavmeshDriver,
}

impl Sub {
    /// Build both sub-drivers from the shared, read-only nav data. `agent_radius` is the
    /// navmesh funnel inset (16.0 matches the pure-navmesh dispatch).
    pub fn new(graph: Arc<NavGraph>, mesh: Arc<NavMesh>, agent_radius: f32) -> Self {
        Self {
            astar: NavigationDriver::new(Arc::clone(&graph)),
            navmesh: NavmeshDriver::new(mesh, agent_radius),
            graph,
        }
    }
}

/// Translate a goal into one the navmesh can consume: the navmesh has no waypoint indices
/// (it ignores [`NavGoal::Waypoint`]), so resolve the index to its world position. Other goal
/// kinds pass through unchanged.
pub(crate) fn goal_to_pos(graph: &NavGraph, goal: &NavGoal) -> NavGoal {
    match goal {
        NavGoal::Waypoint(idx) => NavGoal::Position(Vec3::from(graph.node_pos(*idx))),
        other => other.clone(),
    }
}

/// The world-space position a goal resolves to — used to detect when the goal has changed
/// enough to re-arm a hybrid's backend selection.
pub(crate) fn goal_key(graph: &NavGraph, goal: &NavGoal) -> Vec3 {
    match goal {
        NavGoal::Waypoint(idx) => Vec3::from(graph.node_pos(*idx)),
        NavGoal::Position(p) | NavGoal::Entity(p) => *p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_to_pos_resolves_waypoint_to_node_position() {
        let g = NavGraph::from_raw(vec![[1.0, 2.0, 3.0], [9.0, 8.0, 7.0]], vec![vec![], vec![]]);
        match goal_to_pos(&g, &NavGoal::Waypoint(1)) {
            NavGoal::Position(p) => assert_eq!(p, Vec3::new(9.0, 8.0, 7.0)),
            other => panic!("expected Position, got {other:?}"),
        }
        // Non-waypoint goals pass through.
        match goal_to_pos(&g, &NavGoal::Position(Vec3::new(5.0, 5.0, 5.0))) {
            NavGoal::Position(p) => assert_eq!(p, Vec3::new(5.0, 5.0, 5.0)),
            other => panic!("expected Position, got {other:?}"),
        }
    }
}
