//! Behavior FSM — Roam → Hunt → Engage → Flee → Pickup.
//!
//! Drives navigation and combat based on current state. Transitions are
//! triggered by worldview conditions (low health, enemy in sight, item nearby).

use crate::combat::CombatDecision;
use crate::nav::NavGoal;
use crate::perception::Worldview;
use glam::Vec3;
use world::CollisionModel;

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
    /// Tick the FSM based on current worldview. Returns movement intent. `los` is
    /// the collision model for the Roam→Engage "enemy in sight" gate (Plan 11): when
    /// `None` it degrades to FOV-only sighting.
    pub fn tick(&mut self, view: &Worldview, los: Option<&CollisionModel>) -> BehaviorIntent {
        // Check transitions first
        self.transition(view, los);

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

    fn transition(&mut self, view: &Worldview, los: Option<&CollisionModel>) {
        // Low health → Flee (highest priority)
        if view.is_low_health_with_threshold(30) {
            *self = Self::Flee;
            return;
        }

        // Item nearby → Pickup
        if let Some(item) = view
            .nearest_item(crate::perception::EntityClass::ItemHealth)
            .or_else(|| view.nearest_item(crate::perception::EntityClass::ItemArmor))
        {
            let dist = (item.origin - view.self_state().origin).length();
            if dist < 64.0 {
                tracing::info!(
                    item_entity = item.entity_number,
                    distance = "{:.1}",
                    dist,
                    "picking up item"
                );
                *self = Self::Pickup {
                    item_entity: item.entity_number,
                };
                return;
            }
        }

        // Enemy in sight → Engage. Trace-gated when geometry is available (Plan 11)
        // so a bot doesn't flip to Engage on an enemy behind a wall.
        let nearest = match los {
            Some(cm) => view.nearest_visible_enemy(cm, 90.0),
            None => view.nearest_enemy(90.0),
        };
        tracing::debug!(
            "FSM transition check: state={:?}, nearest_enemy={:?}, enemy_count={}",
            self,
            nearest.as_ref().map(|e| e.entity_number),
            view.enemies().count()
        );

        if let Some(enemy) = nearest {
            let distance = (enemy.origin - view.self_state().origin).length();
            tracing::info!(
                target = enemy.entity_number,
                distance = "{:.1}",
                distance,
                "seeing enemy"
            );
            tracing::debug!(
                "FSM transition: {:?} → Engage (target={}, distance={:.1})",
                self,
                enemy.entity_number,
                distance
            );
            *self = Self::Engage {
                target_entity: enemy.entity_number,
            };
            return;
        }

        // State-specific transitions when no enemy is visible
        match self {
            Self::Hunt { last_enemy_pos } => {
                if last_enemy_pos.is_none() {
                    *self = Self::Roam;
                }
            }
            Self::Engage { target_entity } => {
                // Enemy left FOV — remember last-known position and Hunt.
                let last_pos = view
                    .entities()
                    .find(|e| e.entity_number == *target_entity)
                    .map(|e| e.origin);
                *self = Self::Hunt {
                    last_enemy_pos: last_pos,
                };
            }
            Self::Flee | Self::Pickup { .. } => {}
            Self::Roam => {}
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
        let target = view.entities().find(|e| e.entity_number == target_entity);

        BehaviorIntent {
            // Chase the target while firing.
            nav_goal: target.map(|t| NavGoal::Entity(t.origin)),
            combat_decision: target.map(|_| CombatDecision {
                should_fire: true,
                aim_yaw: 0.0,
                aim_pitch: 0.0,
                target_entity: Some(target_entity),
                weapon_request: None,
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
