//! Navigation driver — A* over the nav graph with stuck recovery.

use glam::Vec3;
use std::sync::Arc;
use world::NavGraph;

const STUCK_THRESHOLD_TICKS: i32 = 30;
const STUCK_MIN_MOVEMENT: f32 = 16.0;

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
    last_position: Option<Vec3>,
    stuck_ticks: i32,
    is_stuck: bool,
}

impl NavigationDriver {
    pub fn new(nav_graph: Arc<NavGraph>) -> Self {
        Self {
            nav_graph,
            current_path: Vec::new(),
            current_waypoint: None,
            last_position: None,
            stuck_ticks: 0,
            is_stuck: false,
        }
    }

    pub fn set_goal(&mut self, goal: NavGoal, from_position: Vec3) {
        let target_waypoint = match goal {
            NavGoal::Waypoint(idx) => Some(idx),
            NavGoal::Position(pos) => self.nav_graph.nearest(&[pos.x, pos.y, pos.z]),
            NavGoal::Entity(pos) => self.nav_graph.nearest(&[pos.x, pos.y, pos.z]),
        };

        if let Some(target) = target_waypoint {
            let nearest = self.nav_graph.nearest(&[from_position.x, from_position.y, from_position.z]);
            if let Some(start) = nearest {
                if let Some(path) = self.nav_graph.path(start, target) {
                    self.current_path = path;
                    self.current_waypoint = self.current_path.first().copied();
                } else {
                    self.current_path.clear();
                    self.current_waypoint = None;
                }
            }
        }
    }

    pub fn current_waypoint(&self) -> Option<usize> {
        self.current_waypoint
    }

    pub fn next_waypoint_direction(&self, from_position: Vec3) -> Option<Vec3> {
        self.current_waypoint.map(|wp_idx| {
            let wp_pos = Vec3::from(self.nav_graph.nodes[wp_idx]);
            (wp_pos - from_position).normalize()
        })
    }

    pub fn update(&mut self, position: Vec3) -> bool {
        if let Some(wp_idx) = self.current_waypoint {
            let wp_pos = Vec3::from(self.nav_graph.nodes[wp_idx]);
            let dist = (wp_pos - position).length();

            if dist < 32.0 {
                let current_idx = self.current_path.iter().position(|&w| w == wp_idx);
                if let Some(idx) = current_idx {
                    if idx + 1 < self.current_path.len() {
                        self.current_waypoint = Some(self.current_path[idx + 1]);
                    } else {
                        self.current_waypoint = None;
                        return true;
                    }
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

    pub fn is_stuck(&self) -> bool {
        self.is_stuck
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
        assert!(STUCK_THRESHOLD_TICKS > 0);
        assert!(STUCK_THRESHOLD_TICKS < 100);
    }

    #[test]
    fn test_stuck_min_movement() {
        assert!(STUCK_MIN_MOVEMENT > 0.0);
        assert!(STUCK_MIN_MOVEMENT < 128.0);
    }

    #[test]
    fn test_stuck_action_variants() {
        let action = StuckAction::Jump;
        assert_eq!(action, StuckAction::Jump);
    }
}
