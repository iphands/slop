//! Perception layer — transforms raw `Frame` data into a usable `Worldview`.
//!
//! Ports the PVS-limited perception model from Eraser/3ZB2: entities not in the
//! current frame's PVS are marked "stale" (not removed), with last-known-position
//! decay. Classification is based on configstrings (CS_MODELS, CS_PLAYERSKINS).

use std::collections::HashMap;

use crate::weapons::Weapon;
use client::parse::{ConfigStrings, CS_MODELS, MAX_MODELS};
use glam::Vec3;
use q2proto::{Frame, PlayerState};

// `CS_MODELS` (32, `shared.h:1203`) and `MAX_MODELS` (256, `shared.h:187`) come from
// `client::parse` as the single source of truth; the model configstring range is
// `CS_MODELS..CS_MODELS + MAX_MODELS` = `32..288` (`CS_SOUNDS` starts at 288).

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

/// Cap on derived velocity (u/s). Fastest real motion: rocket 700, rocket-jump
/// bounce ~1000. Above this it is a teleport/spawnbaseline/multi-frame gap, and
/// `aim_direction` fails safe on `None` (treats the target as stationary) where a
/// bogus 9000 u/s value would lead a shot metres past the target.
const MAX_TRACK_VELOCITY: f32 = 2000.0;

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
    /// Measured velocity in u/s, capped at [`MAX_TRACK_VELOCITY`] (over → zero, a
    /// teleport). Two sources, because the wire only carries one of them:
    /// - **Non-players**: the wire's `old_origin`→`origin` delta over the span it covers
    ///   ([`Frame::velocity_dt_for`]: the frame gap when client-filled, one tick when the
    ///   server sent it). Always `Some`; a stationary or baseline update measures a real
    ///   zero.
    /// - **Players**: from the [`MotionTracker`] — origin now minus origin when last
    ///   seen, over that serverframe gap. `None` on first sight or after leaving PVS
    ///   for more than [`MotionTracker::MAX_GAP`] frames ("no idea"; aim treats it as
    ///   stationary). A player's wire `old_origin` always equals its `origin` (see the
    ///   tracker doc), so the wire path would report every player as standing still.
    pub velocity: Option<Vec3>,
    pub angles: Vec3,
    pub health: Option<i32>,
    pub weapon: Option<i32>,
    /// The weapon this player is **holding**, inferred from its VWep wield model (`modelindex2`
    /// → CS_MODELS, Plan 28). `None` for non-players, when VWep is off, or an unknown model — we
    /// never guess. Lets `main` read the matchup (hold range vs a railgunner, rush a shotgunner).
    pub held_weapon: Option<Weapon>,
    /// The frame this entity was last seen in a packet (for staleness reporting).
    pub last_seen_frame: i32,
    /// Always `false` for entities present in the current frame: the Q2 delta
    /// stream only transmits **visible** entities, and [`Worldview::from_frame`]
    /// builds its list from that list, so every entity here was seen *this* frame.
    /// A `true` value can therefore only come from a test helper
    /// (`entities_mut`); cross-PVS memory — knowing where an entity we lost sight
    /// of was last — needs a persistent per-bot store, which is not yet built.
    /// Until then, treat this as a permanent `false` at runtime.
    pub is_stale: bool,
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
    /// Previous frame's health for detecting damage.
    prev_health: i32,
}

/// The modelindex→classification lookup, rebuilt only when the server rewrites the
/// model configstring range.
///
/// This is the second half of fix #5: `Worldview::from_frame` used to re-derive the
/// whole table every frame. It is static for the life of a level — the model range is
/// written only during level load, by `SV_SpawnServer` (`sv_init.c:316,331`) and by
/// gamecode's `gi.ModelIndex`→`SV_ModelIndex` (`sv_init.c:133`) as entities spawn, and
/// never touched again until the next map, while the configstrings that do churn
/// mid-match (`CS_PLAYERSKINS` on every spawn/respawn, `CS_LIGHTS` on a toggled mover)
/// sit outside the range, keyed by `Conn::cs_revision` rather than `Conn::model_revision`.
/// Rebuilding per frame recomputed ~100 identical classifications per bot per tick.
///
/// The arrays are inline, not `Vec`: the range is fixed at `MAX_MODELS`, so the whole
/// cache is a 528-byte struct with no heap allocation (both element types are 1-byte
/// discriminants — measured, not assumed), and a rebuild is a fixed-size refill instead
/// of two allocations plus a rescan. It is `Copy`-free by design — a per-bot cache
/// should be moved, not duplicated.
#[derive(Debug, Clone)]
pub struct ModelTable {
    /// The [`client::Conn::model_revision`] this table was built against. `u64::MAX`
    /// means "never built" so a first `ensure` always rebuilds without a bool flag.
    revision: u64,
    model_to_class: [EntityClass; MAX_MODELS],
    model_to_weapon: [Option<Weapon>; MAX_MODELS],
    /// How many times the table has actually been rebuilt, read through
    /// [`ModelTable::build_count`]. Observability, not behaviour: the freshness
    /// guarantee (steady frames ⇒ one build) is the whole point of this type, and
    /// nothing else in the gate can see it. Private behind an accessor so nothing
    /// outside [`ModelTable::rebuild`] can scribble it, while the accessor — on a type
    /// re-exported from the crate root — keeps it externally reachable and so out of
    /// the `dead_code` lint that `-D warnings` would otherwise raise.
    build_count: u32,
}

impl ModelTable {
    /// Number of times this table has been rebuilt. `0` means it has never been
    /// built; steady-state frames must leave it unmoved.
    pub fn build_count(&self) -> u32 {
        self.build_count
    }
}

impl Default for ModelTable {
    fn default() -> Self {
        Self {
            // Sentinel, NOT a derived 0: a fresh `Conn::model_revision()` IS 0, so a
            // `revision: 0` would make the first `ensure` a no-op and serve an
            // all-`Unknown` table until a model write happened to bump the counter —
            // silent, frame-plausible, green-suite. No real revision reaches u64::MAX.
            revision: u64::MAX,
            model_to_class: [EntityClass::Unknown; MAX_MODELS],
            model_to_weapon: [None; MAX_MODELS],
            build_count: 0,
        }
    }
}

impl ModelTable {
    /// A table built from `configstrings`, for callers that have no long-lived cache
    /// to feed (tests, cold paths). Prefer [`Worldview::from_frame_cached`] in a
    /// bot loop.
    fn built(configstrings: &ConfigStrings) -> Self {
        let mut t = Self::default();
        t.rebuild(configstrings);
        t
    }

    /// Re-derive both lookups from the model configstring range.
    ///
    /// The reset-to-`Unknown` refill is REQUIRED, not conservative tidying: across a
    /// level change the new map may not precache a model the old one did, and the
    /// stale slot would then classify the new level's entities with the old level's
    /// answer. It is a 512-byte refill against a rebuild that used to cost two heap
    /// allocations plus ~100 `to_lowercase` allocations, so it is not the cost anyone
    /// worries about.
    fn rebuild(&mut self, configstrings: &ConfigStrings) {
        self.model_to_class = [EntityClass::Unknown; MAX_MODELS];
        self.model_to_weapon = [None; MAX_MODELS];
        for (i, model_str) in configstrings.iter() {
            if i < CS_MODELS {
                continue;
            }
            let modelindex = i - CS_MODELS;
            if modelindex < MAX_MODELS {
                if let Some(class) = classify_model(model_str) {
                    self.model_to_class[modelindex] = class;
                }
                if let Some(w) = Weapon::from_wield_model(model_str) {
                    self.model_to_weapon[modelindex] = Some(w);
                }
            }
        }
        self.build_count = self.build_count.saturating_add(1);
    }

    /// Rebuild iff `model_revision` (from [`client::Conn::model_revision`]) differs
    /// from the one this table was built against. Cheap steady-state path: one
    /// `u64` compare.
    fn ensure(&mut self, configstrings: &ConfigStrings, model_revision: u64) {
        if model_revision != self.revision {
            self.rebuild(configstrings);
            self.revision = model_revision;
        }
    }
}

/// Per-bot memory of where each **player** entity was last seen, so a player's velocity
/// can be measured across frames. It has to live here because the wire cannot carry it:
/// Yamagi writes players as "newentities" on every delta (`sv_entities.c:99-102`),
/// which forces `U_OLDORIGIN` (`movemsg.c:348`) carrying the game's own `s.old_origin`
/// — and `G_RunFrame` stamps that equal to `s.origin` (`g_main.c:453`) AFTER the
/// client's move for that frame has already run (`sv_main.c:411` ReadPackets →
/// `:448` RunGameFrame → `:451` Send). So every player packet has
/// `old_origin == origin`, and the wire delta reads a 300 u/s strafe as zero.
/// Non-players are not force-newentity: their `old_origin` is the delta source's
/// origin (`cl_parse.c:159`) and the wire delta is right, so they stay on that path.
///
/// Keyed by entity number, which for players is the stable client slot. Bounded by
/// `MAX_EDICTS`, so it never needs pruning; a level change must [`MotionTracker::clear`]
/// it (the serverframe counter restarts, and the gap check below rejects a backwards
/// counter anyway, so a missed clear degrades to one `None`, never to a bogus value).
#[derive(Debug, Default)]
pub struct MotionTracker {
    last_seen: HashMap<i32, Sighting>,
}

/// One tracked entity: where and when it was last seen, plus the velocity derived on
/// that sighting — kept so a tick that rebuilds the view from the SAME frame (ticks
/// and frames are not phase-locked) hands back the same answer instead of `None`.
#[derive(Debug, Clone, Copy)]
struct Sighting {
    origin: Vec3,
    serverframe: i32,
    velocity: Option<Vec3>,
}

impl MotionTracker {
    /// Largest serverframe gap across which a velocity is still derived. Past it the
    /// entity was out of PVS and the straight line from "then" to "now" is not motion.
    pub const MAX_GAP: i32 = 5;

    /// Record entity `number` at `origin` on `serverframe` and return its velocity
    /// (u/s, uncapped) from the previous sighting. Re-observing the same serverframe
    /// returns the velocity derived when that frame was first seen. `None` on first
    /// sight, after more than [`Self::MAX_GAP`] frames unseen, or if the frame counter
    /// went backwards (level change without a [`Self::clear`]).
    fn observe(&mut self, number: i32, origin: Vec3, serverframe: i32) -> Option<Vec3> {
        let velocity = match self.last_seen.get(&number) {
            Some(prev) if prev.serverframe == serverframe => return prev.velocity,
            Some(prev) => {
                let gap = serverframe - prev.serverframe;
                (1..=Self::MAX_GAP)
                    .contains(&gap)
                    .then(|| (origin - prev.origin) / (gap as f32 * 0.1))
            }
            None => None,
        };
        self.last_seen.insert(
            number,
            Sighting {
                origin,
                serverframe,
                velocity,
            },
        );
        velocity
    }

    /// Forget every sighting — call on a level change, when entity numbers and the
    /// serverframe counter both start over.
    pub fn clear(&mut self) {
        self.last_seen.clear();
    }
}

impl Worldview {
    /// Build a Worldview from a Frame, configstrings, and our player number.
    /// `playernum` is the 0-based slot from `svc_serverdata`; our entity = playernum+1.
    ///
    /// Rebuilds the model table from scratch on every call and tracks no motion, so
    /// every player's `velocity` is `None`. A bot loop should hold a [`ModelTable`] +
    /// [`MotionTracker`] and call [`Worldview::from_frame_cached`] instead — the table
    /// is static for the life of a level, so re-deriving it per frame is the cost fix
    /// #5 removes, and player velocity only exists across frames. This wrapper stays
    /// because ~25 test and cold-path callers pass a throwaway `ConfigStrings`, and
    /// forcing them to thread caches buys nothing.
    pub fn from_frame(frame: &Frame, configstrings: &ConfigStrings, playernum: i16) -> Self {
        Self::assemble(
            frame,
            configstrings,
            playernum,
            &ModelTable::built(configstrings),
            &mut MotionTracker::default(),
        )
    }

    /// [`Worldview::from_frame`] reusing a caller-owned [`ModelTable`] — refreshed only
    /// when `model_revision` (from [`client::Conn::model_revision`]) has moved since
    /// the table was last built; the steady-state cost is one `u64` compare — and a
    /// caller-owned [`MotionTracker`], which is what gives players a velocity at all.
    pub fn from_frame_cached(
        frame: &Frame,
        configstrings: &ConfigStrings,
        playernum: i16,
        models: &mut ModelTable,
        model_revision: u64,
        motion: &mut MotionTracker,
    ) -> Self {
        models.ensure(configstrings, model_revision);
        Self::assemble(frame, configstrings, playernum, models, motion)
    }

    fn assemble(
        frame: &Frame,
        configstrings: &ConfigStrings,
        playernum: i16,
        models: &ModelTable,
        motion: &mut MotionTracker,
    ) -> Self {
        // Parse self state from playerstate
        let mut self_state = SelfState::from_playerstate(&frame.playerstate);
        // Resolve the held weapon from the `gunindex` view-model configstring (Plan 36):
        // gunindex is a 1-based CS_MODELS index naming the first-person weapon model.
        //
        // This stays a live `configstrings` read rather than a table lookup, on
        // purpose: `model_to_weapon` is keyed by the THIRD-person wield models
        // (`#w_railgun.md2`), while `gunindex` names a FIRST-person view model
        // (`v_rail/tris.md2`), and `from_wield_model` returns `None` for the latter.
        // Serving our own held weapon from that table would read "no weapon" while the
        // bot holds a railgun. Two different name families, two different resolvers.
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
                // `.get`, never `[..]`: `modelindex` is an `i32` off the wire, and a
                // negative or oversized value must classify Unknown rather than panic
                // in the frame-decode path.
                models
                    .model_to_class
                    .get(entity_state.modelindex as usize)
                    .copied()
                    .unwrap_or(EntityClass::Unknown)
            };

            let origin = Vec3::from(entity_state.origin);
            let raw_velocity =
                if matches!(class, EntityClass::EnemyPlayer | EntityClass::AllyPlayer) {
                    // Players: the wire's `old_origin` ALWAYS equals `origin` for them (see
                    // `MotionTracker`), so the only measurement is across frames we keep.
                    motion.observe(entity_state.number, origin, frame.serverframe)
                } else {
                    // Everything else comes straight off the wire: one subtraction gives
                    // the measured motion over the span `old_origin` actually covers — the
                    // delta's frame gap when the client filled it, one server tick when the
                    // server sent it (`Frame::velocity_dt_for`; dividing by a wall-clock dt
                    // mixes units and re-inflates the gap).
                    Some(
                        (origin - Vec3::from(entity_state.old_origin))
                            / frame.velocity_dt_for(entity_state),
                    )
                };
            // Over-cap values are a teleport or a >5-tick stale delta: zero, because
            // lead prediction fails safe at zero and detonates at 9000 u/s.
            let velocity = raw_velocity.map(|v| {
                if v.length() > MAX_TRACK_VELOCITY {
                    Vec3::ZERO
                } else {
                    v
                }
            });
            let perceived = PerceivedEntity {
                entity_number: entity_state.number,
                class,
                origin,
                velocity,
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
                        models
                            .model_to_weapon
                            .get(entity_state.modelindex2 as usize)
                            .copied()
                            .flatten()
                    })
                    .flatten(),
                last_seen_frame: frame.serverframe,
                // Present this frame ⇒ not stale by definition (see the field doc).
                is_stale: false,
            };

            entities.push(perceived);
        }

        Worldview {
            frame_number: frame.serverframe,
            self_state,
            entities,
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
        let origin = self.self_state.origin;

        self.enemies()
            .filter(|e| self.in_fov(e.origin, fov_degrees))
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
    ///
    /// `fov_degrees` is a half-angle: 90° = the front hemisphere. Any value ≥ 180° means
    /// "all directions" — the strict `dot > cos(180°) = -1` test would exclude a target at
    /// EXACTLY 180° (dot = −1), so pain-widened acquisition (Plan 49) short-circuits it.
    fn in_fov(&self, target: Vec3, fov_degrees: f32) -> bool {
        if fov_degrees >= 180.0 {
            return true;
        }
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

    /// The velocity cap must bracket real motion: above the fastest projectile
    /// (`Weapon::projectile_speed`) so no real threat is clipped, far below any
    /// teleport so gaps are still rejected. Asserts against the weapons table
    /// rather than a copy-pasted constant that drifts from what it guards.
    #[test]
    fn velocity_cap_brackets_real_motion() {
        for w in [
            Weapon::Blaster,
            Weapon::GrenadeLauncher,
            Weapon::RocketLauncher,
            Weapon::Hyperblaster,
            Weapon::Bfg10k,
        ] {
            let speed = w.projectile_speed().unwrap_or(0.0);
            assert!(
                MAX_TRACK_VELOCITY > speed,
                "{w:?} travels at {speed} u/s; the cap would clip it into zero velocity"
            );
        }
        const { assert!(MAX_TRACK_VELOCITY < 5000.0) }; // cap must reject teleports
    }

    /// Regression guard for a471a2450: NON-PLAYER velocity must come from the wire's
    /// `old_origin`→`origin` delta. Before this fix `velocity` was structurally
    /// always `None` (the lookup searched the freshly-built vec), which silently
    /// killed all rocket/grenade dodging. (Players are NOT on this path — see
    /// `player_velocity_comes_from_the_tracker_not_the_wire`.)
    #[test]
    fn velocity_derives_from_old_origin() {
        use q2proto::{EntityState, Frame};
        // deltaframe 100 → a 1-tick delta, so velocity_dt_for() is the nominal 0.1 s.
        let frame = Frame {
            serverframe: 101,
            deltaframe: 100,
            entities: vec![
                // Mover (a projectile): last transmitted at (100,0,0), now (120,0,0)
                // → 20u/0.1s.
                EntityState {
                    number: 2,
                    origin: [120.0, 0.0, 0.0],
                    old_origin: [100.0, 0.0, 0.0],
                    modelindex: 1,
                    ..Default::default()
                },
                // Stationary (old_origin == origin after the carry-through stamp):
                // a real zero, not a dropout — aim/lead treat zero and None alike,
                // and danger.rs ignores a zero-velocity entity either way.
                EntityState {
                    number: 3,
                    origin: [500.0, 0.0, 0.0],
                    old_origin: [500.0, 0.0, 0.0],
                    modelindex: 1,
                    ..Default::default()
                },
            ],
            ..Frame::default()
        };
        let cs = ConfigStrings::default();
        let view = Worldview::from_frame(&frame, &cs, 0);
        let mut ents = view.entities();
        let mover = ents.next().expect("mover");
        let v = mover.velocity.expect("moving entity must have velocity");
        assert!(
            (v.x - 200.0).abs() < 0.01 && v.y.abs() < 0.01,
            "20u over 0.1s must read 200 u/s along x, got {v}"
        );
        let still = ents.next().expect("stationary");
        assert_eq!(
            still.velocity,
            Some(Vec3::ZERO),
            "unchanged origin → real zero"
        );
    }

    /// A teleport (or any multi-frame-gap artifact) must NOT produce a huge
    /// velocity: a fake 9000 u/s vector would lead a rocket 900u past the
    /// target. Over the cap collapses to zero — fail safe.
    #[test]
    fn teleport_velocity_is_capped_to_zero() {
        use q2proto::{EntityState, Frame};
        let frame = Frame {
            entities: vec![EntityState {
                number: 2,
                origin: [5000.0, 0.0, 0.0],
                old_origin: [0.0, 0.0, 0.0], // 50 km/s
                modelindex: 1,
                ..Default::default()
            }],
            ..Frame::default()
        };
        let view = Worldview::from_frame(&frame, &ConfigStrings::default(), 0);
        assert_eq!(
            view.entities().next().unwrap().velocity,
            Some(Vec3::ZERO),
            "absurd delta must not be reported as motion"
        );
    }

    /// A player entity (`modelindex == 255`) at `origin` on `serverframe`, with the
    /// wire's `old_origin` stamped EQUAL to `origin` — which is what Yamagi sends for
    /// every player on every frame (`sv_entities.c:99-102` force-newentity →
    /// `movemsg.c:348` `U_OLDORIGIN` → `g_main.c:453` `old_origin = origin`, after
    /// the client's move already ran). A fixture with a differing player `old_origin`
    /// is a packet the server cannot produce.
    fn player_frame(serverframe: i32, origin: [f32; 3]) -> q2proto::Frame {
        use q2proto::{EntityState, Frame};
        Frame {
            serverframe,
            deltaframe: serverframe - 1,
            entities: vec![EntityState {
                number: 2,
                origin,
                old_origin: origin,
                modelindex: 255,
                ..Default::default()
            }],
            ..Frame::default()
        }
    }

    fn player_velocity(view: &Worldview) -> Option<Vec3> {
        let e = view.entities().next().expect("player present");
        assert_eq!(e.class, EntityClass::EnemyPlayer);
        e.velocity
    }

    /// The bug a471a2450 left in place: a player's wire `old_origin` equals its
    /// `origin`, so the wire delta reads every player as stationary and projectile
    /// lead against players was still computed against zero. Player velocity must
    /// instead be measured across frames by the caller-owned `MotionTracker`:
    /// first sight is "no idea", the next frame yields the real delta over the
    /// serverframe gap, and a repeat build of the same frame returns the same answer.
    #[test]
    fn player_velocity_comes_from_the_tracker_not_the_wire() {
        let cs = ConfigStrings::default();
        let mut models = ModelTable::default();
        let mut motion = MotionTracker::default();
        let mut build = |sf: i32, origin: [f32; 3]| {
            Worldview::from_frame_cached(
                &player_frame(sf, origin),
                &cs,
                0,
                &mut models,
                1,
                &mut motion,
            )
        };

        // The wire path (what `from_frame` does) says "stationary" for a player that
        // moved 30u this tick — that is the lie the tracker exists to replace.
        let wire_only = Worldview::from_frame(&player_frame(101, [130.0, 0.0, 0.0]), &cs, 0);
        assert_eq!(
            player_velocity(&wire_only),
            None,
            "cold path: no history, no idea"
        );

        assert_eq!(
            player_velocity(&build(100, [100.0, 0.0, 0.0])),
            None,
            "first sight"
        );
        let v = player_velocity(&build(101, [130.0, 0.0, 0.0])).expect("second sight");
        assert!(
            (v.x - 300.0).abs() < 0.01 && v.y.abs() < 0.01,
            "30u over one 0.1s tick = 300 u/s along x, got {v}"
        );
        // Same frame rebuilt on a tick that brought no new frame: same answer, not None.
        let again = player_velocity(&build(101, [130.0, 0.0, 0.0])).expect("repeat frame");
        assert_eq!(
            again, v,
            "re-observing the same serverframe keeps its velocity"
        );
        // A 2-frame gap divides by the gap: 60u over 0.2s is still 300 u/s.
        let v2 = player_velocity(&build(103, [190.0, 0.0, 0.0])).expect("gap of 2");
        assert!((v2.x - 300.0).abs() < 0.01, "gap-scaled: got {v2}");
    }

    /// Out of PVS and back: the straight line from "where it was 3 s ago" to "here"
    /// is not motion, so past `MAX_GAP` the tracker answers `None` (aim: stationary),
    /// and so does a backwards serverframe (level change without a `clear`). A teleport
    /// inside the gap is caught by the same cap as everything else.
    #[test]
    fn player_velocity_is_none_after_a_pvs_gap_and_capped_on_teleport() {
        let cs = ConfigStrings::default();
        let mut models = ModelTable::default();
        let mut motion = MotionTracker::default();
        // The tracker is a parameter, not a capture, so `clear` can be called below.
        let mut build = |motion: &mut MotionTracker, sf: i32, origin: [f32; 3]| {
            Worldview::from_frame_cached(&player_frame(sf, origin), &cs, 0, &mut models, 1, motion)
        };

        build(&mut motion, 100, [0.0, 0.0, 0.0]);
        let gap = MotionTracker::MAX_GAP + 1;
        assert_eq!(
            player_velocity(&build(&mut motion, 100 + gap, [600.0, 0.0, 0.0])),
            None,
            "unseen for {gap} frames: not a measurement"
        );
        // Re-armed by that sighting: the next frame measures again.
        assert!(player_velocity(&build(&mut motion, 100 + gap + 1, [620.0, 0.0, 0.0])).is_some());
        // Frame counter went backwards (new level): no bogus cross-level delta.
        assert_eq!(
            player_velocity(&build(&mut motion, 5, [0.0, 0.0, 0.0])),
            None
        );
        // Teleport within the gap: over the cap collapses to zero, same as the wire path.
        assert_eq!(
            player_velocity(&build(&mut motion, 6, [5000.0, 0.0, 0.0])),
            Some(Vec3::ZERO),
            "50 km/s is a teleport, fail safe to stationary"
        );
        // `clear` forgets everything: the next sighting is a first sight again.
        motion.clear();
        assert_eq!(
            player_velocity(&build(&mut motion, 7, [5010.0, 0.0, 0.0])),
            None
        );
    }

    /// The other half of the span rule: when `U_OLDORIGIN` was on the wire the server's
    /// `old_origin` is exactly one tick old however wide the frame gap is — an entity
    /// ENTERING the PVS on a 2-frame gap, or any entity of an uncompressed frame. Dividing
    /// those by the gap under-reports: a 70u rocket step would read 350 u/s, and the
    /// old per-frame `velocity_dt` did exactly that for every uncompressed frame.
    #[test]
    fn explicit_old_origin_spans_one_tick_regardless_of_gap() {
        use q2proto::{EntityState, Frame};
        let rocket = |old_origin_explicit: bool| EntityState {
            number: 2,
            origin: [170.0, 0.0, 0.0],
            old_origin: [100.0, 0.0, 0.0], // 70u
            modelindex: 1,
            old_origin_explicit,
            ..Default::default()
        };
        let vx = |frame: &Frame| {
            Worldview::from_frame(frame, &ConfigStrings::default(), 0)
                .entities()
                .next()
                .unwrap()
                .velocity
                .unwrap()
                .x
        };
        // Entering the PVS on a 2-frame gap: the wire carried old_origin → 1 tick.
        let entering = Frame {
            serverframe: 103,
            deltaframe: 101,
            entities: vec![rocket(true)],
            ..Frame::default()
        };
        assert!(
            (vx(&entering) - 700.0).abs() < 0.01,
            "70u / 0.1s, got {}",
            vx(&entering)
        );
        // Same numbers client-filled → the 2-tick span applies.
        let carried = Frame {
            entities: vec![rocket(false)],
            ..entering.clone()
        };
        assert!(
            (vx(&carried) - 350.0).abs() < 0.01,
            "70u / 0.2s, got {}",
            vx(&carried)
        );
        // Uncompressed frame: every entity is server-stamped → 1 tick, never 2.
        let uncompressed = Frame {
            serverframe: 103,
            deltaframe: -1,
            entities: vec![rocket(true)],
            ..Frame::default()
        };
        assert!(
            (vx(&uncompressed) - 700.0).abs() < 0.01,
            "got {}",
            vx(&uncompressed)
        );
    }

    /// Velocity must divide by the DELTA's frame span (`serverframe - deltaframe`),
    /// not a nominal tick: the same 20u step across a 2-tick delta is 100 u/s, and
    /// dividing by 0.1 would report 200 — the inflation that made the first pass of
    /// this fix wrong (a 3-frame gap reads a strafe as a rocket).
    #[test]
    fn velocity_divides_by_delta_span() {
        use q2proto::{EntityState, Frame};
        let frame = Frame {
            serverframe: 103,
            deltaframe: 101, // 2-tick delta
            entities: vec![EntityState {
                number: 2,
                origin: [120.0, 0.0, 0.0],
                old_origin: [100.0, 0.0, 0.0],
                modelindex: 1,
                ..Default::default()
            }],
            ..Frame::default()
        };
        let view = Worldview::from_frame(&frame, &ConfigStrings::default(), 0);
        assert!((view.entities().next().unwrap().velocity.unwrap().x - 100.0).abs() < 0.01);
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

    /// Configstrings for the cache tests: a weapon whose name ALSO resolves through
    /// the wield matcher, an item, a first-person view model, and a model that
    /// classifies as nothing.
    fn probe_configstrings() -> ConfigStrings {
        let mut cs = ConfigStrings::default();
        cs.set(CS_MODELS + 1, "models/w_shotgun.md2");
        cs.set(CS_MODELS + 2, "models/item_health_small.md2");
        cs.set(CS_MODELS + 3, "models/weapons/v_rail/tris.md2");
        cs.set(CS_MODELS + 9, "maps/q2dm1/submodels/face0.md2");
        cs
    }

    /// Entities matching [`probe_configstrings`], incl. the `modelindex == 255`
    /// player sentinel, an out-of-range index, and `gunindex` set so the live
    /// view-model read runs on every call.
    fn probe_frame() -> Frame {
        use q2proto::EntityState;
        let mut frame = Frame {
            serverframe: 7,
            deltaframe: 6,
            entities: vec![
                EntityState {
                    number: 2,
                    modelindex: 1,
                    ..Default::default()
                },
                EntityState {
                    number: 3,
                    modelindex: 2,
                    ..Default::default()
                },
                EntityState {
                    number: 4,
                    modelindex: 255,
                    ..Default::default()
                },
                EntityState {
                    number: 5,
                    modelindex: 9,
                    ..Default::default()
                },
                EntityState {
                    number: 6,
                    modelindex: 4_000_000, // off the wire, must not panic
                    ..Default::default()
                },
            ],
            ..Frame::default()
        };
        frame.playerstate.gunindex = 3;
        frame
    }

    fn classified(view: &Worldview) -> Vec<(i32, EntityClass, Option<Weapon>)> {
        view.entities()
            .map(|e| (e.entity_number, e.class, e.held_weapon))
            .collect()
    }

    /// The whole point of the type, and the proof-of-win that replaces a benchmark
    /// because the gate runs it forever: ten frames of an unchanged level rebuild
    /// ONCE. Before fix #5 this cost a rescan + ~100 `to_lowercase` allocs per frame.
    #[test]
    fn model_table_builds_once_across_steady_frames() {
        let cs = probe_configstrings();
        let frame = probe_frame();
        let mut models = ModelTable::default();

        for tick in 0..10 {
            let view = Worldview::from_frame_cached(
                &frame,
                &cs,
                0,
                &mut models,
                5,
                &mut MotionTracker::default(),
            );
            assert_eq!(
                view.self_state.held_weapon,
                Some(Weapon::Railgun),
                "held weapon on tick {tick}"
            );
        }
        assert_eq!(
            models.build_count(),
            1,
            "ten frames at one model_revision must derive the table once"
        );
    }

    /// Caching must change how OFTEN the table is derived, never WHAT it says: the
    /// cold wrapper and the cached entry point must agree on every entity. The
    /// expected values are spelled out so a change to `classify_model` fails this too,
    /// rather than two paths agreeing while both went wrong.
    #[test]
    fn cached_path_agrees_with_cold_build_on_content() {
        let cs = probe_configstrings();
        let frame = probe_frame();
        let cold = Worldview::from_frame(&frame, &cs, 0);
        let mut models = ModelTable::default();
        let warm = Worldview::from_frame_cached(
            &frame,
            &cs,
            0,
            &mut models,
            1,
            &mut MotionTracker::default(),
        );

        assert_eq!(
            cold.self_state.held_weapon, warm.self_state.held_weapon,
            "our held weapon comes from the live view-model read, not the cache"
        );
        let expected = vec![
            (2, EntityClass::ItemWeapon, None),
            (3, EntityClass::ItemHealth, None),
            (4, EntityClass::EnemyPlayer, None),
            (5, EntityClass::Unknown, None),
            (6, EntityClass::Unknown, None),
        ];
        assert_eq!(
            classified(&cold),
            expected,
            "fixture classifies as documented"
        );
        assert_eq!(classified(&warm), expected, "cached path says the same");
    }

    /// A level change wipes the model table, so a rebuild must not INHERIT the
    /// previous level's answers. `rebuild` refills every slot with `Unknown` before
    /// rescanning for exactly this reason — drop that refill and index 1 here keeps
    /// reading `ItemWeapon` on a map where it is a brush face. The `build_count == 2`
    /// half is what a real `Conn` guarantees via `reset_configstrings`.
    #[test]
    fn rebuild_does_not_inherit_the_previous_levels_classifications() {
        let frame = probe_frame();
        let mut models = ModelTable::default();
        let first = Worldview::from_frame_cached(
            &frame,
            &probe_configstrings(),
            0,
            &mut models,
            1,
            &mut MotionTracker::default(),
        );
        assert_eq!(models.build_count(), 1);
        assert_eq!(
            classified(&first)[0],
            (2, EntityClass::ItemWeapon, None),
            "sanity: index 1 IS a weapon on the first level"
        );

        // New level: same index, different model, and nothing else precached.
        let mut second_map = ConfigStrings::default();
        second_map.set(CS_MODELS + 1, "maps/q2dm3/submodels/face0.md2");
        let second = Worldview::from_frame_cached(
            &frame,
            &second_map,
            0,
            &mut models,
            2,
            &mut MotionTracker::default(),
        );

        assert_eq!(models.build_count(), 2, "a moved revision rebuilds");
        assert_eq!(
            classified(&second)[0],
            (2, EntityClass::Unknown, None),
            "the old level's ItemWeapon must NOT survive at modelindex 1"
        );
    }

    /// The same refill guarantee for the OTHER half of the table. An enemy's held
    /// weapon is resolved through `model_to_weapon` by `modelindex2`, so a stale
    /// weapon slot makes bots report a weapon the new map never precached — and
    /// `classify_model` cannot catch it, because the class and the weapon come from
    /// separate arrays. The existing refill test never sets `modelindex2`, so without
    /// this one deleting `model_to_weapon`'s refill line keeps the suite green.
    #[test]
    fn rebuild_does_not_inherit_the_previous_levels_weapons() {
        use q2proto::EntityState;
        // A player entity (modelindex 255) wielding whatever CS_MODELS+1 names.
        let frame = Frame {
            serverframe: 7,
            deltaframe: 6,
            entities: vec![EntityState {
                number: 2,
                modelindex: 255,
                modelindex2: 1,
                ..Default::default()
            }],
            ..Frame::default()
        };
        let mut models = ModelTable::default();

        let mut first_map = ConfigStrings::default();
        first_map.set(CS_MODELS + 1, "#w_railgun.md2");
        let first = Worldview::from_frame_cached(
            &frame,
            &first_map,
            0,
            &mut models,
            1,
            &mut MotionTracker::default(),
        );
        assert_eq!(
            classified(&first)[0],
            (2, EntityClass::EnemyPlayer, Some(Weapon::Railgun)),
            "sanity: index 1 IS a railgun on the first level"
        );

        // New level: same index, and it is not a weapon any more.
        let mut second_map = ConfigStrings::default();
        second_map.set(CS_MODELS + 1, "models/items/md2/key.md2");
        let second = Worldview::from_frame_cached(
            &frame,
            &second_map,
            0,
            &mut models,
            2,
            &mut MotionTracker::default(),
        );
        assert_eq!(
            classified(&second)[0],
            (2, EntityClass::EnemyPlayer, None),
            "the old level's Railgun must NOT survive at modelindex2 1"
        );
    }
}
