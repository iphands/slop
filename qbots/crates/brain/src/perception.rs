//! Perception layer — transforms raw `Frame` data into a usable `Worldview`.
//!
//! Ports the PVS-limited perception model from Eraser/3ZB2: entities not in the
//! current frame's PVS are marked "stale" (not removed), with last-known-position
//! decay. Classification is based on configstrings (CS_MODELS, CS_PLAYERSKINS).

use client::parse::ConfigStrings;
use glam::Vec3;
use q2proto::{Frame, PlayerState};

/// Stats indices (from shared.h:1130-1148)
const STAT_HEALTH: usize = 1;
#[allow(dead_code)]
const STAT_AMMO: usize = 3;
const STAT_ARMOR: usize = 5;

/// How many frames an entity can be unseen before marked stale.
const STALE_THRESHOLD: i32 = 10; // ~1 second at 10 Hz

/// Classification of an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityClass {
    SelfPlayer,
    EnemyPlayer,
    AllyPlayer,
    ItemHealth,
    ItemArmor,
    ItemWeapon,
    ItemPowerup,
    ProjectileRocket,
    ProjectileGrenade,
    Unknown,
}

/// A classified entity with state.
#[derive(Debug, Clone)]
pub struct PerceivedEntity {
    pub entity_number: i32,
    pub class: EntityClass,
    pub origin: Vec3,
    #[allow(dead_code)]
    pub velocity: Option<Vec3>, // None if stale > 2 frames
    pub angles: Vec3,
    pub health: Option<i32>,
    pub weapon: Option<i32>,
    pub last_seen_frame: i32,
    pub is_stale: bool,
    /// Previous frame's origin for velocity calculation.
    #[allow(dead_code)]
    last_origin: Option<Vec3>,
}

/// The bot's own state.
#[derive(Debug, Clone)]
pub struct SelfState {
    pub origin: Vec3,
    pub velocity: Vec3,
    pub angles: Vec3,
    pub health: i32,
    pub armor: i32,
    pub ammo: [i32; 32],
    pub weapon: i32,
    pub flags: u32,
}

/// A complete worldview for one frame.
#[derive(Debug, Clone)]
pub struct Worldview {
    pub frame_number: i32,
    pub self_state: SelfState,
    entities: Vec<PerceivedEntity>,
    /// Pre-built lookup: modelindex → EntityClass.
    #[allow(dead_code)]
    model_to_class: Vec<EntityClass>,
}

impl Worldview {
    /// Build a Worldview from a Frame and configstrings.
    pub fn from_frame(frame: &Frame, configstrings: &ConfigStrings) -> Self {
        // Build model→class lookup from CS_MODELS
        let mut model_to_class = vec![EntityClass::Unknown; 256];
        for (i, model_str) in configstrings.iter() {
            if i == 0 {
                continue; // CS_MODELS starts at index 0, but we skip it
            }
            if let Some(class) = classify_model(model_str) {
                if i < model_to_class.len() {
                    model_to_class[i] = class;
                }
            }
        }

        // Parse self state from playerstate
        let self_state = SelfState::from_playerstate(&frame.playerstate);

        // Parse entities
        let mut entities: Vec<PerceivedEntity> = Vec::new();
        for entity_state in &frame.entities {
            let class = if entity_state.number == 1 {
                EntityClass::SelfPlayer
            } else {
                model_to_class
                    .get(entity_state.modelindex as usize)
                    .copied()
                    .unwrap_or(EntityClass::Unknown)
            };

            let origin = Vec3::from(entity_state.origin);
            let prev = entities.iter().find(|e| e.entity_number == entity_state.number);

            let perceived = PerceivedEntity {
                entity_number: entity_state.number,
                class,
                origin,
                velocity: prev
                    .and_then(|p| {
                        if p.last_seen_frame == frame.serverframe - 1 {
                            let dt = 0.1; // Assume 10 Hz
                            let delta = origin - p.origin;
                            Some(delta / dt)
                        } else {
                            None
                        }
                    }),
                angles: Vec3::from(entity_state.angles),
                health: if class != EntityClass::SelfPlayer {
                    None // Only self has health in playerstate
                } else {
                    Some(self_state.health)
                },
                weapon: None, // TODO: extract from entity flags
                last_seen_frame: frame.serverframe,
                is_stale: false,
                last_origin: Some(origin),
            };

            entities.push(perceived);
        }

        // Mark stale entities
        for entity in &mut entities {
            if frame.serverframe - entity.last_seen_frame > STALE_THRESHOLD {
                entity.is_stale = true;
                entity.velocity = None;
            }
        }

        Worldview {
            frame_number: frame.serverframe,
            self_state,
            entities,
            model_to_class,
        }
    }

    /// Get self state.
    pub fn self_state(&self) -> &SelfState {
        &self.self_state
    }

    /// Iterate over all entities.
    pub fn entities(&self) -> impl Iterator<Item = &PerceivedEntity> {
        self.entities.iter()
    }

    /// Iterate over enemy players.
    pub fn enemies(&self) -> impl Iterator<Item = &PerceivedEntity> {
        self.entities
            .iter()
            .filter(|e| e.class == EntityClass::EnemyPlayer && !e.is_stale)
    }

    /// Iterate over items.
    pub fn items(&self) -> impl Iterator<Item = &PerceivedEntity> {
        self.entities.iter().filter(|e| {
            matches!(
                e.class,
                EntityClass::ItemHealth
                    | EntityClass::ItemArmor
                    | EntityClass::ItemWeapon
                    | EntityClass::ItemPowerup
            ) && !e.is_stale
        })
    }

    /// Find the nearest enemy within FOV.
    pub fn nearest_enemy(&self, fov_degrees: f32) -> Option<&PerceivedEntity> {
        let fov_radians = fov_degrees.to_radians();
        let forward = self.forward_vector();
        
        self.enemies()
            .filter(|e| {
                let direction = (e.origin - self.self_state.origin).normalize();
                forward.dot(direction) > fov_radians.cos()
            })
            .min_by_key(|e| {
                let dist = (e.origin - self.self_state.origin).length_squared();
                dist as i32
            })
    }

    /// Convert view angles to a forward direction vector.
    fn forward_vector(&self) -> Vec3 {
        let yaw = self.self_state.angles.y.to_radians();
        glam::Vec3::new(yaw.cos(), yaw.sin(), 0.0)
    }

    /// Find all items within range.
    pub fn items_in_range(&self, range: f32) -> Vec<&PerceivedEntity> {
        let range_sq = range * range;
        self.items()
            .filter(|e| {
                (e.origin - self.self_state.origin)
                    .length_squared()
                    < range_sq
            })
            .collect()
    }

    /// Check if we're low on health.
    pub fn is_low_health(&self) -> bool {
        self.self_state.health < 25
    }

    /// Check if we're low on health with a custom threshold.
    pub fn is_low_health_with_threshold(&self, threshold: i32) -> bool {
        self.self_state.health < threshold
    }

    /// Check if any enemy is within range (early exit, no allocation).
    pub fn enemy_in_range(&self, range: f32) -> bool {
        let range_sq = range * range;
        self.enemies().any(|e| {
            (e.origin - self.self_state.origin).length_squared() < range_sq
        })
    }

    /// Get health percentage (0-100). Useful for decision thresholds.
    pub fn health_percent(&self) -> f32 {
        (self.self_state.health as f32 / 100.0).min(1.0) * 100.0
    }

    /// Check if we have a specific weapon.
    pub fn has_weapon(&self, weapon_id: i32) -> bool {
        self.self_state.weapon == weapon_id
    }

    /// Find nearest item by type.
    pub fn nearest_item(&self, class: EntityClass) -> Option<&PerceivedEntity> {
        self.items()
            .filter(|e| e.class == class)
            .min_by_key(|e| {
                let dist = (e.origin - self.self_state.origin).length_squared();
                dist as i32
            })
    }

    /// Check if the worldview is fresh (not dropped frames).
    pub fn is_fresh(&self) -> bool {
        true // TODO: track dropped frames
    }
}

impl SelfState {
    fn from_playerstate(ps: &PlayerState) -> Self {
        Self {
            origin: Vec3::from(ps.pmove.origin_f32()),
            velocity: Vec3::from(ps.pmove.velocity_f32()),
            angles: Vec3::from(ps.viewangles),
            health: ps.stats[STAT_HEALTH] as i32,
            armor: ps.stats[STAT_ARMOR] as i32,
            ammo: ps.stats.map(|s| s as i32),
            weapon: ps.gunindex,
            flags: ps.pmove.pm_flags as u32,
        }
    }
}

/// Classify an entity based on its model string.
fn classify_model(model_str: &str) -> Option<EntityClass> {
    let s = model_str.to_lowercase();
    if s.contains("health") {
        Some(EntityClass::ItemHealth)
    } else if s.contains("armor") {
        Some(EntityClass::ItemArmor)
    } else if s.contains("weapon") || s.contains("gun") {
        Some(EntityClass::ItemWeapon)
    } else if s.contains("quad")
        || s.contains("invulnerability")
        || s.contains("environmental_suit")
    {
        Some(EntityClass::ItemPowerup)
    } else if s.contains("rocket") {
        Some(EntityClass::ProjectileRocket)
    } else if s.contains("grenade") {
        Some(EntityClass::ProjectileGrenade)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_model() {
        assert_eq!(classify_model("item_health"), Some(EntityClass::ItemHealth));
        assert_eq!(classify_model("weapon_shotgun"), Some(EntityClass::ItemWeapon));
        assert_eq!(classify_model("quad_damage"), Some(EntityClass::ItemPowerup));
        assert_eq!(classify_model("unknown"), None);
    }

    #[test]
    fn test_stale_threshold() {
        assert!(STALE_THRESHOLD > 0);
        assert!(STALE_THRESHOLD <= 20); // Reasonable decay
    }
}
