//! Navigation driver — A* over the nav graph with stuck recovery.

use glam::Vec3;
use std::sync::Arc;
use world::NavGraph;

const STUCK_THRESHOLD_TICKS: i32 = 30;
const STUCK_MIN_MOVEMENT: f32 = 16.0;
/// Hard cap on pursuing a single goal without reaching a waypoint. At 10 Hz
/// this is ~8 s — past it we abandon the goal (Eraser's 4 s give-up is tighter,
/// but we tolerate the slow nav-graph routing). Prevents infinite stale-enemy
/// chases. (`eraser.md` §3 give-up watchdog.)
const GOAL_GIVEUP_TICKS: i32 = 80;
/// Desperate-requery guard (Plan 08 T3): if a risk-weighted path is more than
/// this many × the straight-line distance, the whole region is hot and we drop
/// the overlay rather than pay an absurd detour ("desperate re-query with W_d=0").
const DEGEN_FACTOR: f32 = 5.0;
/// Only apply the degeneracy guard past this straight-line distance, so a tiny
/// goal (where the ratio is meaningless) can't trigger it.
const DEGEN_MIN_STRAIGHT: f32 = 256.0;

#[derive(Debug, Clone)]
pub enum NavGoal {
    Waypoint(usize),
    Position(Vec3),
    Entity(Vec3),
}

#[derive(Clone)]
pub struct NavigationDriver {
    nav_graph: Arc<NavGraph>,
    current_path: Vec<usize>,
    current_waypoint: Option<usize>,
    /// Goal node from the last successful plan; used to skip redundant replans.
    last_goal_node: Option<usize>,
    last_position: Option<Vec3>,
    stuck_ticks: i32,
    is_stuck: bool,
    /// Ticks since the current goal was set without reaching a waypoint. Drives
    /// the give-up watchdog.
    goal_age_ticks: i32,
    /// True for one tick after the watchdog abandons a stale goal.
    goal_abandoned: bool,
    /// Per-node additive cost overlay for risk-weighted A\* (Plan 08 T3): each
    /// edge leaving node `n` costs `base + overlay[n]`. Set each tick from the
    /// heatmap (`W_d·danger − W_p·popularity`); `None` = unweighted routing.
    risk_overlay: Option<Vec<f32>>,
}

impl NavigationDriver {
    pub fn new(nav_graph: Arc<NavGraph>) -> Self {
        Self {
            nav_graph,
            current_path: Vec::new(),
            current_waypoint: None,
            last_goal_node: None,
            last_position: None,
            stuck_ticks: 0,
            is_stuck: false,
            goal_age_ticks: 0,
            goal_abandoned: false,
            risk_overlay: None,
        }
    }

    /// Install the risk overlay consumed by the next `set_goal` (Plan 08 T3).
    /// Build it from the heatmap as `W_d·danger − W_p·popularity`. Cheap to call
    /// every tick — `set_goal` only re-runs A\* when the goal changes.
    pub fn set_risk_overlay(&mut self, overlay: Vec<f32>) {
        self.risk_overlay = Some(overlay);
    }

    /// Drop the risk overlay, returning to unweighted routing. Use as the
    /// "desperate re-query" when danger-routing wedges the bot (stuck recovery).
    pub fn clear_risk_overlay(&mut self) {
        self.risk_overlay = None;
    }

    /// Set (or update) the navigation goal. Replans the A* path only when the goal
    /// changes or the current path is exhausted. `set_goal` is safe to call every tick.
    pub fn set_goal(&mut self, goal: NavGoal, from_position: Vec3) {
        let target_waypoint = match goal {
            NavGoal::Waypoint(idx) => Some(idx),
            NavGoal::Position(pos) => {
                tracing::debug!("nav goal Position({:.1},{:.1},{:.1})", pos.x, pos.y, pos.z);
                self.nav_graph.nearest(&[pos.x, pos.y, pos.z])
            }
            NavGoal::Entity(pos) => {
                tracing::debug!("nav goal Entity({:.1},{:.1},{:.1})", pos.x, pos.y, pos.z);
                self.nav_graph.nearest(&[pos.x, pos.y, pos.z])
            }
        };

        let Some(target) = target_waypoint else {
            tracing::warn!("nav goal: no reachable waypoint found");
            return;
        };

        // Don't replan if goal unchanged and we still have a waypoint to follow.
        if self.last_goal_node == Some(target) && self.current_waypoint.is_some() {
            return;
        }
        self.last_goal_node = Some(target);

        let Some(start) =
            self.nav_graph
                .nearest(&[from_position.x, from_position.y, from_position.z])
        else {
            return;
        };

        if let Some(path) = self.plan_path(start, target) {
            self.commit_path(path);
        } else {
            // Different components: path within start's component toward target.
            let target_pos = self.nav_graph.nodes[target];
            if let Some(alt) = self.nav_graph.nearest_reachable_from(start, &target_pos) {
                if let Some(path) = self.plan_path(start, alt) {
                    self.commit_path(path);
                    return;
                }
            }
            tracing::warn!("nav path not found from {} to {}", start, target);
            self.current_path.clear();
            self.current_waypoint = None;
        }
    }

    /// Plan a path `start → target`, applying the risk overlay (Plan 08 T3) with
    /// a desperate fallback: weighted A\* first; if that yields no path or a
    /// degenerate (absurdly long) detour — the whole region is hot — re-query
    /// with the overlay dropped ("desperate re-query with W_d=0").
    fn plan_path(&self, start: usize, target: usize) -> Option<Vec<usize>> {
        // No overlay → plain unweighted A*.
        let Some(overlay) = self.risk_overlay.as_deref() else {
            return self.nav_graph.path(start, target);
        };
        let path = self.nav_graph.path_weighted(start, target, overlay)?;
        // Degeneracy guard.
        let straight = Vec3::from(self.nav_graph.nodes[start])
            .distance(Vec3::from(self.nav_graph.nodes[target]));
        if straight > DEGEN_MIN_STRAIGHT {
            let len = self.nav_graph.path_len(&path);
            if len > straight * DEGEN_FACTOR {
                tracing::debug!(
                    straight,
                    len,
                    "risk-weighted path degenerate; retrying unweighted"
                );
                return self.nav_graph.path(start, target);
            }
        }
        tracing::debug!("nav path found (weighted): {} nodes", path.len());
        Some(path)
    }

    /// Store the path and set the first *meaningful* waypoint (skip the start node,
    /// which is where the bot already is).
    fn commit_path(&mut self, path: Vec<usize>) {
        self.current_path = path;
        // path[0] == start (our position); skip it so we aim at the next node.
        let first = if self.current_path.len() > 1 {
            self.current_path[1]
        } else {
            // Single-node path: already at goal.
            self.current_path[0]
        };
        self.current_waypoint = Some(first);
        self.goal_age_ticks = 0;
        self.goal_abandoned = false;
    }

    pub fn current_waypoint(&self) -> Option<usize> {
        self.current_waypoint
    }

    pub fn next_waypoint_direction(&self, from_position: Vec3) -> Option<Vec3> {
        self.current_waypoint.and_then(|wp_idx| {
            let wp_pos = Vec3::from(self.nav_graph.nodes[wp_idx]);
            let delta = wp_pos - from_position;
            if delta.length_squared() < 1e-6 {
                return None; // avoid NaN from normalizing a zero vector
            }
            Some(delta.normalize())
        })
    }

    pub fn update(&mut self, position: Vec3) -> bool {
        self.goal_abandoned = false;

        if let Some(wp_idx) = self.current_waypoint {
            let wp_pos = Vec3::from(self.nav_graph.nodes[wp_idx]);
            let dist = (wp_pos - position).length();

            if dist < 64.0 {
                // Reached a waypoint — reset the give-up clock and advance.
                self.goal_age_ticks = 0;
                let current_idx = self.current_path.iter().position(|&w| w == wp_idx);
                if let Some(idx) = current_idx {
                    if idx + 1 < self.current_path.len() {
                        self.current_waypoint = Some(self.current_path[idx + 1]);
                    } else {
                        // Reached the goal; allow set_goal to plan a new one.
                        self.current_waypoint = None;
                        self.last_goal_node = None;
                        return true;
                    }
                }
            } else {
                // Still pursuing — age the goal toward the give-up cap.
                self.goal_age_ticks += 1;
                if self.goal_age_ticks > GOAL_GIVEUP_TICKS {
                    tracing::debug!(
                        age = self.goal_age_ticks,
                        "goal give-up: abandoning stale goal"
                    );
                    self.current_path.clear();
                    self.current_waypoint = None;
                    self.last_goal_node = None;
                    self.goal_age_ticks = 0;
                    self.goal_abandoned = true;
                }
            }
        }

        if let Some(last_pos) = self.last_position {
            let movement = (position - last_pos).length();
            if movement < STUCK_MIN_MOVEMENT {
                self.stuck_ticks += 1;
                if self.stuck_ticks > STUCK_THRESHOLD_TICKS && !self.is_stuck {
                    self.is_stuck = true;
                }
            } else {
                self.stuck_ticks = 0;
                self.is_stuck = false;
            }
        }
        self.last_position = Some(position);

        false
    }

    /// True for one tick after the give-up watchdog abandoned a stale goal. The
    /// caller can use this to fall back to roaming instead of re-issuing the same
    /// (unreachable) goal.
    pub fn goal_abandoned(&self) -> bool {
        self.goal_abandoned
    }

    pub fn is_stuck(&self) -> bool {
        self.is_stuck
    }

    /// Force the next `set_goal` to replan from scratch, even if the goal is
    /// unchanged. Call after clearing an obstacle so the bot doesn't re-attempt
    /// the same wedged waypoint.
    pub fn force_replan(&mut self) {
        self.current_path.clear();
        self.current_waypoint = None;
        self.last_goal_node = None;
    }

    pub fn reset_stuck(&mut self) {
        self.stuck_ticks = 0;
        self.is_stuck = false;
    }

    pub fn stuck_recovery(&mut self) -> StuckAction {
        self.is_stuck = false;
        self.stuck_ticks = 0;
        StuckAction::Jump
    }

    pub fn path_length(&self) -> usize {
        self.current_path.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StuckAction {
    Jump,
    BackOff,
    Repath,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stuck_threshold() {
        const { assert!(STUCK_THRESHOLD_TICKS > 0) };
        const { assert!(STUCK_THRESHOLD_TICKS < 100) };
    }

    #[test]
    fn test_stuck_min_movement() {
        const { assert!(STUCK_MIN_MOVEMENT > 0.0) };
        const { assert!(STUCK_MIN_MOVEMENT < 128.0) };
    }

    #[test]
    fn test_stuck_action_variants() {
        let action = StuckAction::Jump;
        assert_eq!(action, StuckAction::Jump);
    }

    #[test]
    fn risk_overlay_detours_driver_around_danger() {
        use std::sync::Arc;
        // Diamond: 0=A 1=B 2=C 3=D. A→C direct via B (200), longer via D (282).
        let g = Arc::new(NavGraph::from_raw(
            vec![
                [0.0, 0.0, 0.0],
                [0.0, 100.0, 0.0],
                [0.0, 200.0, 0.0],
                [100.0, 100.0, 0.0],
            ],
            vec![
                vec![(1, 100.0), (3, 141.0)],
                vec![(0, 100.0), (2, 100.0)],
                vec![(1, 100.0), (3, 141.0)],
                vec![(0, 141.0), (2, 141.0)],
            ],
        ));
        let mut nav = NavigationDriver::new(Arc::clone(&g));

        // No overlay → A→C routes via B (waypoint 1).
        nav.set_goal(NavGoal::Waypoint(2), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(nav.current_waypoint(), Some(1), "unweighted picks B");

        // Danger at B → replan routes via D (waypoint 3).
        nav.set_risk_overlay(vec![0.0, 1000.0, 0.0, 0.0]);
        nav.force_replan();
        nav.set_goal(NavGoal::Waypoint(2), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(nav.current_waypoint(), Some(3), "danger at B detours via D");

        // Dropping the overlay restores the direct route.
        nav.clear_risk_overlay();
        nav.force_replan();
        nav.set_goal(NavGoal::Waypoint(2), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(
            nav.current_waypoint(),
            Some(1),
            "cleared overlay returns to B"
        );
    }
}
