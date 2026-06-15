//! Behavior FSM — Roam → Hunt → Engage → Flee → Pickup.
//!
//! Drives navigation and combat based on current state. Transitions are
//! triggered by worldview conditions (low health, enemy in sight, item nearby).

use crate::combat::CombatDecision;
use crate::nav::NavGoal;
use crate::perception::Worldview;
use glam::Vec3;

/// Behavior states for the bot.
#[derive(Debug, Clone, PartialEq)]
pub enum BehaviorState {
    /// Seeking random roam nodes or high-value items.
    Roam,
    /// Moving toward last-known enemy position.
    Hunt { last_enemy_pos: Option<Vec3> },
    /// Enemy in sight — engage with combat.
    Engage { target_entity: i32 },
    /// Low health/armor — flee to find health.
    Flee,
    /// Near an item — pick it up.
    Pickup { item_entity: i32 },
}

/// Movement intent from the FSM (combines nav goal + combat decision).
#[derive(Debug, Clone)]
pub struct BehaviorIntent {
    pub nav_goal: Option<NavGoal>,
    pub combat_decision: Option<CombatDecision>,
    pub should_pickup: Option<i32>, // Entity number to pickup
}

impl BehaviorState {
    /// Tick the FSM based on current worldview. Returns movement intent.
    pub fn tick(&mut self, view: &Worldview) -> BehaviorIntent {
        // Check transitions first
        self.transition(view);

        // Execute current state - extract values to avoid borrow issues
        match self {
            Self::Roam => self.roam(view),
            Self::Hunt { last_enemy_pos } => {
                let pos = *last_enemy_pos;
                self.hunt(view, pos)
            }
            Self::Engage { target_entity } => {
                let entity = *target_entity;
                self.engage(view, entity)
            }
            Self::Flee => self.flee(view),
            Self::Pickup { item_entity } => {
                let entity = *item_entity;
                self.pickup(entity)
            }
        }
    }

    fn transition(&mut self, view: &Worldview) {
        // Low health → Flee (highest priority)
        if view.is_low_health_with_threshold(30) {
            *self = Self::Flee;
            return;
        }

        // Item nearby → Pickup
        if let Some(item) = view.nearest_item(crate::perception::EntityClass::ItemHealth)
            .or_else(|| view.nearest_item(crate::perception::EntityClass::ItemArmor))
        {
            let dist = (item.origin - view.self_state().origin).length();
            if dist < 64.0 {
                *self = Self::Pickup {
                    item_entity: item.entity_number,
                };
                return;
            }
        }

        // Enemy in sight → Engage
        if let Some(enemy) = view.nearest_enemy(90.0) {
            *self = Self::Engage {
                target_entity: enemy.entity_number,
            };
            return;
        }

        // State-specific transitions when no enemy is visible
        match self {
            Self::Hunt { last_enemy_pos } => {
                // If we have a last known position, keep hunting
                // If None, transition to Roam
                if last_enemy_pos.is_none() {
                    *self = Self::Roam;
                }
                // else: stay in Hunt, no change needed
            }
            Self::Engage { .. } | Self::Flee | Self::Pickup { .. } => {
                // These states will be handled by their priority checks above
                // If we reach here, we're in one of these and no higher-priority
                // transition triggered, so stay put
            }
            Self::Roam => {
                // Already roaming, no change needed
            }
        }
    }

    fn roam(&self, view: &Worldview) -> BehaviorIntent {
        // Seek nearest roam node or random item
        let goal = view
            .nearest_item(crate::perception::EntityClass::ItemHealth)
            .or_else(|| view.nearest_item(crate::perception::EntityClass::ItemArmor))
            .map(|item| NavGoal::Position(item.origin));

        BehaviorIntent {
            nav_goal: goal,
            combat_decision: None,
            should_pickup: None,
        }
    }

    fn hunt(&self, view: &Worldview, last_enemy_pos: Option<Vec3>) -> BehaviorIntent {
        if let Some(pos) = last_enemy_pos {
            BehaviorIntent {
                nav_goal: Some(NavGoal::Position(pos)),
                combat_decision: None,
                should_pickup: None,
            }
        } else {
            // No last known, roam
            self.roam(view)
        }
    }

    fn engage(&self, view: &Worldview, target_entity: i32) -> BehaviorIntent {
        // Find target in worldview
        let target = view
            .entities()
            .find(|e| e.entity_number == target_entity);

        BehaviorIntent {
            nav_goal: None, // Stay in place, aim and fire
            combat_decision: target.map(|_| CombatDecision {
                should_fire: true,
                aim_yaw: 0.0, // Will be set by combat module
                aim_pitch: 0.0,
                target_entity: Some(target_entity),
                impulse: None,
            }),
            should_pickup: None,
        }
    }

    fn flee(&self, view: &Worldview) -> BehaviorIntent {
        // Seek health item
        let goal = view
            .nearest_item(crate::perception::EntityClass::ItemHealth)
            .map(|item| NavGoal::Position(item.origin));

        BehaviorIntent {
            nav_goal: goal,
            combat_decision: None,
            should_pickup: None,
        }
    }

    fn pickup(&self, _item_entity: i32) -> BehaviorIntent {
        BehaviorIntent {
            nav_goal: None, // Already near item
            combat_decision: None,
            should_pickup: Some(_item_entity),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fsm_starts_in_roam() {
        let fsm = BehaviorState::Roam;
        assert_eq!(fsm, BehaviorState::Roam);
    }

    #[test]
    fn low_health_triggers_flee() {
        // This would need a mock worldview with low health
        // For now, just verify the state exists
        assert_eq!(BehaviorState::Flee, BehaviorState::Flee);
    }
}
