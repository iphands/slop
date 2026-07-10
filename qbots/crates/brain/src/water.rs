//! Water-state detection (Plan 40).
//!
//! The Q2 wire protocol does not carry the player's `waterlevel`, but the brain owns the
//! [`CollisionModel`] and its own origin, so we recompute it exactly the way `pmove` does
//! (`PM_CategorizePosition`, `pmove.c:765-790`): sample `CONTENTS_WATER` at three heights —
//! feet, mid, eye — and count how many (bottom-up) are submerged. Only `CONTENTS_WATER`
//! counts; lava/slime are deadly and never swum (Plan 39 sampled swim nodes the same way).

use glam::Vec3;
use world::{CollisionModel, CONTENTS_WATER};

/// Standing hull `mins[2]` (`VEC_HULL_MIN.z`); the feet sample sits 1u above it.
const MINS_Z: f32 = -24.0;
/// Standard standing `viewheight` (`pmove.c` / `client.c`); the eye sample sits here.
const VIEWHEIGHT: f32 = 22.0;

/// Vertical delta (units) that maps to full `intent.up` thrust while swimming (Plan 40).
pub const SWIM_VERT_SCALE: f32 = 32.0;
/// View pitch (deg, negative = up) forced during a water-exit climb-out. Q2 grants the
/// water-jump boost only when `viewangles[PITCH] <= -15` + forward + a blocked path
/// (`pmove.c:414`); -20 clears that gate with margin.
pub const EXIT_LOOKUP_PITCH: f32 = -20.0;
/// Ticks to stay in water-exit mode once started, so the bot doesn't oscillate at the lip.
pub const EXIT_HYSTERESIS_TICKS: u32 = 12;

/// Waterlevel ∈ {0,1,2,3} at `origin` (Plan 40), mirroring `PM_CategorizePosition`:
/// `0` dry, `1` feet wet, `2` waist-deep (**swimming**), `3` head under (fully submerged).
pub fn water_level(cm: &CollisionModel, origin: Vec3) -> u8 {
    let sample2 = VIEWHEIGHT - MINS_Z; // eye offset above mins (≈46)
    let sample1 = sample2 / 2.0; // mid offset (≈23)
    let at =
        |dz: f32| cm.point_contents(&[origin.x, origin.y, origin.z + dz]) & CONTENTS_WATER != 0;
    if !at(MINS_Z + 1.0) {
        return 0;
    }
    if !at(MINS_Z + sample1) {
        return 1;
    }
    if !at(MINS_Z + sample2) {
        return 2;
    }
    3
}

/// True once the bot is waterborne enough to swim (waist-deep or more): `waterlevel >= 2`.
/// At this depth Q2 switches to `PM_WaterMove`, so the brain must drive vertical thrust.
#[inline]
pub fn is_swimming(level: u8) -> bool {
    level >= 2
}

/// Q2's air budget while fully submerged: `air_finished = level.time + 12` while
/// `waterlevel < 3` (`vendor/yquake2/src/game/player/view.c:763`); past it at level 3 the
/// server deals escalating drown damage (`view.c:863-866`).
pub const AIR_BUDGET_SECS: f32 = 12.0;
/// Safety margin (Plan 32): treat air as critical this many seconds before the server would —
/// covers observation-start skew (we count from *observed* level 3) plus surfacing slop.
pub const AIR_SAFETY_MARGIN_SECS: f32 = 2.0;

/// Sustained vertical swim speed (u/s) for time-to-surface estimates. Measured from Plan 40's
/// live logs: q2dm1 railgun dive, z 238→434 (196u) in ~46 frames ≈ 4.6s → ~43 u/s net vertical
/// while also moving horizontally; pure up-thrust is faster, so 60 u/s is a conservative planning
/// number that overestimates time-to-surface (errs toward surfacing early).
pub const SWIM_UP_SPEED: f32 = 60.0;

/// The air read the surface-seek gate consumes (Plan 32).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AirState {
    /// Estimated seconds of air left (12 − time submerged; the full 12 when not submerged).
    pub remaining_secs: f32,
    /// Air is past the budget — the server is (or is about to start) dealing drown damage.
    pub drowning: bool,
}

/// Client-side mirror of the server's air clock (Plan 32 T1). The wire doesn't carry
/// `air_finished`, but we compute `water_level` ourselves each tick, so a clock keyed to our own
/// level-3 observation tracks the server to within a tick. `on_unexplained_damage` re-syncs when
/// the server's clock ran ahead of ours (we took drown damage we didn't predict).
#[derive(Debug, Clone, Copy, Default)]
pub struct AirClock {
    /// Seconds spent continuously at `water_level == 3`; `None` while breathing is possible.
    submerged_secs: Option<f32>,
}

impl AirClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance one tick. `eyes_under` = `water_level == 3` this frame; `dt` = seconds.
    pub fn tick(&mut self, eyes_under: bool, dt: f32) -> AirState {
        if eyes_under {
            let t = self.submerged_secs.unwrap_or(0.0) + dt.clamp(0.0, 0.5);
            self.submerged_secs = Some(t);
        } else {
            // Surfaced (even level 2 = mouth above water): the server resets air instantly.
            self.submerged_secs = None;
        }
        let remaining = AIR_BUDGET_SECS - self.submerged_secs.unwrap_or(0.0);
        AirState {
            remaining_secs: remaining,
            drowning: remaining <= 0.0,
        }
    }

    /// The server says we're drowning (health dropped underwater with no other explanation) —
    /// clamp our clock to "out of air" so the surface-seek engages immediately (Plan 32 Risk #1).
    pub fn on_unexplained_damage(&mut self) {
        self.submerged_secs = Some(self.submerged_secs.unwrap_or(0.0).max(AIR_BUDGET_SECS));
    }

    /// Air is critical: surface NOW. `time_to_surface` is the caller's estimate of seconds needed
    /// to reach breathable water (vertical distance / [`SWIM_UP_SPEED`]).
    pub fn must_surface(&self, time_to_surface: f32) -> bool {
        let remaining = AIR_BUDGET_SECS - self.submerged_secs.unwrap_or(0.0);
        self.submerged_secs.is_some() && remaining <= time_to_surface + AIR_SAFETY_MARGIN_SECS
    }
}

/// Estimated seconds to reach breathable water by swimming straight up (Plan 32 T2): scan upward
/// from the eye for the first non-water sample (32u steps, 640u cap) and divide by
/// [`SWIM_UP_SPEED`]. A solid ceiling also ends the scan — conservative for covered tunnels (the
/// swim-path exit logic, which drives toward dry nodes, remains the smarter route; this estimate
/// only feeds the "how urgent is air" gate).
pub fn time_to_surface(cm: &CollisionModel, origin: Vec3) -> f32 {
    let eye = origin.z + VIEWHEIGHT;
    let mut dz = 0.0;
    while dz <= 640.0 {
        if cm.point_contents(&[origin.x, origin.y, eye + dz]) & CONTENTS_WATER == 0 {
            return dz / SWIM_UP_SPEED;
        }
        dz += 32.0;
    }
    640.0 / SWIM_UP_SPEED
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A z-layered world: solid below z=0, water 0..120, air above (in the central channel).
    fn pool() -> CollisionModel {
        world::water_channel_world()
    }

    #[test]
    fn levels_track_depth() {
        let cm = pool();
        // Deep: origin high enough that feet/mid/eye are all in water (water tops at z=120).
        // origin.z=60 → feet=37, mid=59, eye=82 — all in (0,120) water.
        assert_eq!(water_level(&cm, Vec3::new(0.0, 0.0, 60.0)), 3);
        // Dry side ledge (x=100, no water): level 0.
        assert_eq!(water_level(&cm, Vec3::new(100.0, 0.0, 30.0)), 0);
        // Surface: origin.z=110 → feet=87 (water), mid=109 (water), eye=132 (>120, air) → 2.
        assert_eq!(water_level(&cm, Vec3::new(0.0, 0.0, 110.0)), 2);
    }

    #[test]
    fn is_swimming_threshold() {
        assert!(!is_swimming(0));
        assert!(!is_swimming(1));
        assert!(is_swimming(2));
        assert!(is_swimming(3));
    }

    #[test]
    fn time_to_surface_scales_with_depth() {
        let cm = pool(); // water 0..120 in the central channel
                         // Deep (origin z=60, eye 82): first air sample ~64u up → ~1.1s at 60 u/s.
        let deep = time_to_surface(&cm, Vec3::new(0.0, 0.0, 60.0));
        // Shallower (origin z=90, eye 112): air ~one step up → faster.
        let shallow = time_to_surface(&cm, Vec3::new(0.0, 0.0, 90.0));
        assert!(
            deep > shallow,
            "deeper → longer to surface ({deep} vs {shallow})"
        );
        assert!(deep < 3.0, "the pool is shallow in absolute terms");
    }

    // ── AirClock (Plan 32 T1) ──────────────────────────────────────────────────────────────

    #[test]
    fn air_depletes_while_submerged_and_resets_on_surface() {
        let mut c = AirClock::new();
        // Dry: full budget, not drowning, no must_surface.
        let s = c.tick(false, 0.1);
        assert_eq!(s.remaining_secs, AIR_BUDGET_SECS);
        assert!(!s.drowning);
        assert!(!c.must_surface(10.0), "not submerged → never must-surface");
        // Submerge for 5s → ~7s left.
        for _ in 0..50 {
            c.tick(true, 0.1);
        }
        let s = c.tick(true, 0.1);
        assert!((s.remaining_secs - (AIR_BUDGET_SECS - 5.1)).abs() < 0.01);
        assert!(!s.drowning);
        // One frame at the surface (level 2 counts as breathing) → full reset.
        let s = c.tick(false, 0.1);
        assert_eq!(s.remaining_secs, AIR_BUDGET_SECS);
    }

    #[test]
    fn drowning_past_budget() {
        let mut c = AirClock::new();
        for _ in 0..130 {
            c.tick(true, 0.1); // 13s under
        }
        let s = c.tick(true, 0.1);
        assert!(s.drowning);
        assert!(s.remaining_secs <= 0.0);
    }

    #[test]
    fn must_surface_accounts_for_travel_time_and_margin() {
        let mut c = AirClock::new();
        for _ in 0..50 {
            c.tick(true, 0.1); // 5s under → 7s air left
        }
        // 7s left, 2s margin: a 4s swim-up still fits (7 > 4+2)…
        assert!(!c.must_surface(4.0));
        // …but a 6s swim-up doesn't (7 <= 6+2) → surface NOW.
        assert!(c.must_surface(6.0));
    }

    #[test]
    fn unexplained_damage_resyncs_to_drowning() {
        let mut c = AirClock::new();
        c.tick(true, 0.1); // just submerged — our clock thinks we have ~12s
        c.on_unexplained_damage(); // server disagrees (we took drown damage)
        let s = c.tick(true, 0.1);
        assert!(s.drowning, "server-observed drown damage clamps the clock");
        assert!(c.must_surface(0.0));
    }
}
