//! Perception layer — transforms raw `Frame` data into a usable `Worldview`.
//!
//! Ports the PVS-limited perception model from Eraser/3ZB2: entities not in the
//! current frame's PVS are marked "stale" (not removed), with last-known-position
//! decay. Classification is based on configstrings (CS_MODELS, CS_PLAYERSKINS).

use crate::weapons::Weapon;
use client::parse::ConfigStrings;
use glam::Vec3;
use q2proto::{Frame, PlayerState};

/// Configstring index where the models table starts (`CS_MODELS`, `shared.h:1193`).
const CS_MODELS: usize = 32;

/// `CS_PLAYERSKINS` — start of the per-client infostring table (`shared.h:1208`).
/// Derived for yquake2 (MAX_CLIENTS = MAX_MODELS = MAX_SOUNDS = MAX_IMAGES =
/// MAX_LIGHTSTYLES = MAX_ITEMS = 256): `CS_MODELS(32) + 256·5 = 1312`.
/// Validated against `MAX_CONFIGSTRINGS = 2080` (CS_GENERAL = 1312 + 256 = 1568;
/// + MAX_GENERAL(512) = 2080 ✓ — see `context/distilled.md`).
pub const CS_PLAYERSKINS: usize = 1312;
/// `MAX_CLIENTS` (`shared.h:184`) — bounds valid client slots for name lookup.
const MAX_CLIENTS: usize = 256;

/// Stats indices (from shared.h:1130-1148)
const STAT_HEALTH: usize = 1;
/// `STAT_AMMO` (`shared.h`) — the **held** weapon's ammo count (Q2's HUD ammo box).
/// The wire carries no free per-weapon inventory; this is the only ammo we see.
pub const STAT_AMMO: usize = 3;
const STAT_ARMOR: usize = 5;
/// `STAT_FRAGS` — our frag count (`hud.c`). Incremented by the server on kills.
const STAT_FRAGS: usize = 14;

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
    /// The weapon this player is **holding**, inferred from its VWep wield model (`modelindex2`
    /// → CS_MODELS, Plan 28). `None` for non-players, when VWep is off, or an unknown model — we
    /// never guess. Lets `main` read the matchup (hold range vs a railgunner, rush a shotgunner).
    pub held_weapon: Option<Weapon>,
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
    pub frags: i32,
    pub ammo: [i32; 32],
    pub weapon: i32,
    pub flags: u32,
    /// The weapon we are currently **holding**, resolved from the `gunindex` view-model
    /// configstring ([`Weapon::from_view_model`]). `None` before the model table loads or for
    /// an unrecognized view model. This is qbots' wire-visible proxy for Q3's "best owned
    /// weapon" (see [`crate::q3char`]).
    pub held_weapon: Option<Weapon>,
}

impl SelfState {
    /// Ammo for the **held** weapon (Q2 `STAT_AMMO`). Negative/uninitialized stats clamp to 0.
    pub fn held_ammo(&self) -> i32 {
        self.ammo[STAT_AMMO].max(0)
    }
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
    /// Previous frame's health for detecting damage.
    prev_health: i32,
}

impl Worldview {
    /// Build a Worldview from a Frame, configstrings, and our player number.
    /// `playernum` is the 0-based slot from `svc_serverdata`; our entity = playernum+1.
    pub fn from_frame(frame: &Frame, configstrings: &ConfigStrings, playernum: i16) -> Self {
        // Build modelindex→class lookup. Entity modelindex is 1-based into CS_MODELS,
        // so configstring index CS_MODELS+modelindex maps to model_to_class[modelindex].
        let mut model_to_class = vec![EntityClass::Unknown; 256];
        // Parallel lookup for the VWep wield model (`modelindex2`) → enemy's held weapon (Plan 28).
        let mut model_to_weapon: Vec<Option<Weapon>> = vec![None; 256];
        for (i, model_str) in configstrings.iter() {
            if i < CS_MODELS {
                continue;
            }
            let modelindex = i - CS_MODELS;
            if modelindex < model_to_class.len() {
                if let Some(class) = classify_model(model_str) {
                    model_to_class[modelindex] = class;
                }
                if let Some(w) = Weapon::from_wield_model(model_str) {
                    model_to_weapon[modelindex] = Some(w);
                }
            }
        }

        // Parse self state from playerstate
        let mut self_state = SelfState::from_playerstate(&frame.playerstate);
        // Resolve the held weapon from the `gunindex` view-model configstring (Plan 36):
        // gunindex is a 1-based CS_MODELS index naming the first-person weapon model.
        if self_state.weapon > 0 {
            self_state.held_weapon = configstrings
                .get(CS_MODELS + self_state.weapon as usize)
                .and_then(Weapon::from_view_model);
        }
        let self_entity = (playernum + 1) as i32;

        // Parse entities
        let mut entities: Vec<PerceivedEntity> = Vec::new();
        for entity_state in &frame.entities {
            let class = if entity_state.number == self_entity {
                EntityClass::SelfPlayer
            } else if entity_state.modelindex == 255 {
                // Q2 protocol sentinel: modelindex=255 means "use player skin from
                // CS_PLAYERSKINS" — i.e., this is always a player entity.
                EntityClass::EnemyPlayer
            } else {
                model_to_class
                    .get(entity_state.modelindex as usize)
                    .copied()
                    .unwrap_or(EntityClass::Unknown)
            };

            let origin = Vec3::from(entity_state.origin);
            let prev = entities
                .iter()
                .find(|e| e.entity_number == entity_state.number);

            let perceived = PerceivedEntity {
                entity_number: entity_state.number,
                class,
                origin,
                velocity: prev.and_then(|p| {
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
                // Enemy's held weapon from the VWep wield model (`modelindex2`), Plan 28. Only
                // meaningful for players; a non-weapon `modelindex2` resolves to `None`.
                held_weapon: matches!(class, EntityClass::EnemyPlayer | EntityClass::AllyPlayer)
                    .then(|| {
                        model_to_weapon
                            .get(entity_state.modelindex2 as usize)
                            .copied()
                            .flatten()
                    })
                    .flatten(),
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
            prev_health: 0, // First frame, no previous health to compare
        }
    }

    /// Detect health changes between frames and log damage/death events.
    /// Returns the health delta (negative = damage taken).
    pub fn detect_damage(&mut self) -> Option<i32> {
        if self.prev_health == 0 {
            // First frame, just initialize
            self.prev_health = self.self_state.health;
            tracing::debug!("health initialized to {}", self.prev_health);
            return None;
        }

        let delta = self.self_state.health - self.prev_health;

        if delta != 0 {
            tracing::trace!(
                "health changed: {} -> {} (delta={})",
                self.prev_health,
                self.self_state.health,
                delta
            );
        }

        if delta < 0 {
            // Damage taken
            tracing::info!(
                health_before = self.prev_health,
                health_after = self.self_state.health,
                damage = -delta,
                "being hit"
            );

            if self.self_state.health <= 0 {
                tracing::error!(health = 0, "bot death detected");
            }
        } else if delta > 0 {
            // Health restored (picked up health item)
            tracing::debug!(
                health_before = self.prev_health,
                health_after = self.self_state.health,
                healed = delta,
                "health restored"
            );
        }

        self.prev_health = self.self_state.health;
        Some(delta)
    }

    /// Get self state.
    pub fn self_state(&self) -> &SelfState {
        &self.self_state
    }

    /// Iterate over all entities.
    pub fn entities(&self) -> impl Iterator<Item = &PerceivedEntity> {
        self.entities.iter()
    }

    /// Iterate mutably over all entities (test helpers / classification fixes).
    pub fn entities_mut(&mut self) -> impl Iterator<Item = &mut PerceivedEntity> {
        self.entities.iter_mut()
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
        let origin = self.self_state.origin;

        self.enemies()
            .filter(|e| {
                let direction = (e.origin - origin).normalize();
                forward.dot(direction) > fov_radians.cos()
            })
            .min_by(|a, b| {
                let da = (a.origin - origin).length_squared();
                let db = (b.origin - origin).length_squared();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// The nearest enemy within FOV **with an unobstructed line of sight** (Plan 11).
    /// Same as [`Self::nearest_enemy`] but additionally requires a clear BSP trace
    /// from our eye to the enemy's chest/feet, so a wall between us and a target
    /// disqualifies it. `cm` is the collision model the nav graph was built from.
    pub fn nearest_visible_enemy(
        &self,
        cm: &world::CollisionModel,
        fov_degrees: f32,
    ) -> Option<&PerceivedEntity> {
        let eye = crate::los::eye_origin(self.self_state.origin.into());
        self.enemies()
            .filter(|e| self.in_fov(e.origin, fov_degrees))
            .filter(|e| crate::los::has_los_player(cm, eye, e.origin.into()))
            .min_by(|a, b| {
                let da = (a.origin - self.self_state.origin).length_squared();
                let db = (b.origin - self.self_state.origin).length_squared();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Is `target` within the view FOV cone? (Factored out of `nearest_enemy`.)
    fn in_fov(&self, target: Vec3, fov_degrees: f32) -> bool {
        let origin = self.self_state.origin;
        let dir = target - origin;
        if dir.length_squared() < 1e-6 {
            return true;
        }
        self.forward_vector().dot(dir.normalize()) > fov_degrees.to_radians().cos()
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
            .filter(|e| (e.origin - self.self_state.origin).length_squared() < range_sq)
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
        self.enemies()
            .any(|e| (e.origin - self.self_state.origin).length_squared() < range_sq)
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
        let origin = self.self_state.origin;
        self.items().filter(|e| e.class == class).min_by(|a, b| {
            let da = (a.origin - origin).length_squared();
            let db = (b.origin - origin).length_squared();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
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
            frags: ps.stats[STAT_FRAGS] as i32,
            ammo: ps.stats.map(|s| s as i32),
            weapon: ps.gunindex,
            flags: ps.pmove.pm_flags as u32,
            // Resolved by `Worldview::from_frame` (needs the configstring model table).
            held_weapon: None,
        }
    }
}

/// A player's display name from their `CS_PLAYERSKINS` infostring. `entity_number`
/// is 1-based (the player's slot + 1, as carried in `svc_packetentities`). Used
/// by the heatmap observer to attribute obituary deaths to a victim's name.
/// Returns `None` for non-client entity numbers or unset skin strings.
pub fn player_name(cs: &ConfigStrings, entity_number: i32) -> Option<String> {
    if !(1..=MAX_CLIENTS as i32).contains(&entity_number) {
        return None;
    }
    let idx = CS_PLAYERSKINS + (entity_number - 1) as usize;
    cs.get(idx)
        .and_then(|info| infostring_value(info, "name").map(str::to_owned))
}

/// Read one `\key\value\` pair out of a Q2 infostring. Handles both leading and
/// absent leading backslashes (the skin configstring has none).
fn infostring_value<'a>(info: &'a str, key: &str) -> Option<&'a str> {
    let mut parts = info.split('\\').filter(|s| !s.is_empty());
    while let Some(k) = parts.next() {
        match parts.next() {
            Some(v) if k.eq_ignore_ascii_case(key) => return Some(v),
            // No value for this key (trailing key) → stop.
            _ => {}
        }
    }
    None
}

/// Classify an entity based on its model string.
fn classify_model(model_str: &str) -> Option<EntityClass> {
    let s = model_str.to_lowercase();
    // Player models: "players/male/tris.md2", "players/female/tris.md2", etc.
    if s.starts_with("players/") {
        return Some(EntityClass::EnemyPlayer);
    }
    if s.contains("health") {
        Some(EntityClass::ItemHealth)
    } else if s.contains("armor") {
        Some(EntityClass::ItemArmor)
    } else if s.contains("weapon") || s.contains("w_") || s.contains("gun") {
        Some(EntityClass::ItemWeapon)
    } else if s.contains("quad")
        || s.contains("invulnerability")
        || s.contains("environmental_suit")
    {
        Some(EntityClass::ItemPowerup)
    } else if s.contains("rocket") || s.contains("blaster") || s.contains("bolt") {
        Some(EntityClass::ProjectileRocket)
    } else if s.contains("grenade") {
        Some(EntityClass::ProjectileGrenade)
    } else {
        None
    }
}

/// Classify a **static BSP item entity** by its `classname` (Plan 30). Unlike
/// [`classify_model`] (which reads a live entity's model string), this maps the map file's
/// spawn-entity classnames (`item_*`/`weapon_*`/`ammo_*`, `g_items.c` `itemlist[]`) so the brain
/// knows where resources live even when they are outside PVS. `ammo_*` maps to
/// [`EntityClass::ItemWeapon`] for now (a "re-arm" resource — there is no wire-visible ammo class;
/// Plan 30 T4 refines ammo handling). Returns `None` for non-item classnames (spawns, triggers…).
pub fn classify_item_classname(classname: &str) -> Option<EntityClass> {
    let s = classname.to_ascii_lowercase();
    if s.starts_with("item_health") {
        Some(EntityClass::ItemHealth)
    } else if s.starts_with("item_armor") {
        Some(EntityClass::ItemArmor)
    } else if s.starts_with("weapon_") || s.starts_with("ammo_") {
        Some(EntityClass::ItemWeapon)
    } else if matches!(
        s.as_str(),
        "item_quad"
            | "item_invulnerability"
            | "item_silencer"
            | "item_breather"
            | "item_enviro"
            | "item_adrenaline"
            | "item_power_screen"
            | "item_power_shield"
            | "item_ancient_head"
            | "item_bandolier"
            | "item_pack"
    ) {
        Some(EntityClass::ItemPowerup)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_item_classname() {
        use EntityClass::*;
        assert_eq!(
            classify_item_classname("item_health_mega"),
            Some(ItemHealth)
        );
        assert_eq!(classify_item_classname("item_armor_body"), Some(ItemArmor));
        assert_eq!(classify_item_classname("weapon_railgun"), Some(ItemWeapon));
        assert_eq!(classify_item_classname("ammo_slugs"), Some(ItemWeapon));
        assert_eq!(classify_item_classname("item_quad"), Some(ItemPowerup));
        // Non-items → None (spawn points, world, triggers).
        assert_eq!(classify_item_classname("info_player_deathmatch"), None);
        assert_eq!(classify_item_classname("func_train"), None);
    }

    #[test]
    fn test_classify_model() {
        assert_eq!(classify_model("item_health"), Some(EntityClass::ItemHealth));
        assert_eq!(
            classify_model("weapon_shotgun"),
            Some(EntityClass::ItemWeapon)
        );
        assert_eq!(
            classify_model("quad_damage"),
            Some(EntityClass::ItemPowerup)
        );
        assert_eq!(classify_model("unknown"), None);
    }

    #[test]
    fn test_stale_threshold() {
        const { assert!(STALE_THRESHOLD > 0) };
        const { assert!(STALE_THRESHOLD <= 20) }; // Reasonable decay
    }

    /// LOS-gated selection (Plan 11): the nearer enemy is behind a wall, the
    /// farther one is in the open → `nearest_visible_enemy` picks the open one,
    /// while FOV-only `nearest_enemy` picks the nearer walled one. Self faces yaw
    /// 135° so both enemies sit inside a 90° FOV cone.
    #[test]
    fn nearest_visible_enemy_skips_walled_picks_open() {
        use q2proto::{EntityState, Frame};
        // Wall at x=0 (x<0 solid). Self at (100,0,0) facing 135° (-x,+y).
        let cm = world::CollisionModel::half_space([1.0, 0.0, 0.0], 0.0);
        let mut frame = Frame::default();
        frame.playerstate.pmove.origin = [(100.0 * 8.0) as i16, 0, 0];
        frame.playerstate.viewangles = [0.0, 135.0, 0.0];
        frame.entities = vec![
            // Nearer (~144u), but across the wall → no LOS.
            EntityState {
                number: 2,
                origin: [-20.0, 80.0, 0.0],
                modelindex: 255,
                ..Default::default()
            },
            // Farther (~171u), but in the open (same x>0 side as us) → clear LOS.
            EntityState {
                number: 3,
                origin: [40.0, 160.0, 0.0],
                modelindex: 255,
                ..Default::default()
            },
        ];
        let cs = ConfigStrings::default();
        let view = Worldview::from_frame(&frame, &cs, 0);

        // FOV-only: the nearer walled enemy is "nearest".
        let near = view.nearest_enemy(90.0).expect("an enemy");
        assert_eq!(near.entity_number, 2);

        // LOS-gated: the walled enemy is filtered out → the open one is chosen.
        let vis = view
            .nearest_visible_enemy(&cm, 90.0)
            .expect("a visible enemy");
        assert_eq!(
            vis.entity_number, 3,
            "open enemy chosen over the nearer walled one"
        );
    }

    #[test]
    fn player_name_from_skin_infostring() {
        let mut cs = ConfigStrings::default();
        // entity_number=1 → CS_PLAYERSKINS+0. Infostring with no leading backslash.
        cs.set(CS_PLAYERSKINS, "name\\Killer\\skin\\male/grunt\\hand\\0");
        assert_eq!(player_name(&cs, 1).as_deref(), Some("Killer"));

        // entity_number=2 → CS_PLAYERSKINS+1. Infostring with leading backslash.
        cs.set(CS_PLAYERSKINS + 1, "\\name\\Foe\\skin\\female/cyborg");
        assert_eq!(player_name(&cs, 2).as_deref(), Some("Foe"));

        // Out-of-range / unset → None.
        assert_eq!(player_name(&cs, 0), None);
        assert_eq!(player_name(&cs, -1), None);
        assert_eq!(player_name(&cs, MAX_CLIENTS as i32 + 1), None);
        assert_eq!(player_name(&cs, 5), None); // slot never set
    }
}
