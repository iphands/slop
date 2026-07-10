//! # brain::brains::core — the brain plugin contract (Plan 23)
//!
//! Plan 22 collapsed every per-tick decision into one concrete `brain::Brain`. This module
//! turns that single unit into a **plugin seam**: a `trait Brain` many implementations satisfy,
//! plus the bundled per-tick / per-map I/O types they share. It mirrors the nav layer's
//! `trait Navigator` + `NavMode`/`build_navigator` pattern (the factory lives in the parent
//! `brains` module).
//!
//! `core` is deliberately decoupled from any one brain's internals — notably it does **not**
//! reference `BehaviorState` (main-specific); a brain exposes only a short `status()` label.

use std::sync::Arc;

use world::{CollisionModel, NavGraph};

use glam::Vec3;

use crate::move_ctrl::MovementIntent;
use crate::nav::NavGoal;
use crate::nav_mode::Navigator;
use crate::perception::{EntityClass, Worldview};
use crate::weapons::Weapon;

/// A static item spawn known from the map file (Plan 30). Lets the brain seek resources by their
/// *map-known* locations — health, armor, weapons, ammo, powerups — not just what is in PVS this
/// frame (a human knows the mega is around the corner). Built once per map from the BSP entity
/// lump; `nav_node` is the nearest A* graph node (for A*-distance scoring), `None` if off-graph.
#[derive(Debug, Clone, Copy)]
pub struct MapItem {
    /// Resource category (health/armor/weapon/powerup), from `classify_item_classname`.
    pub class: EntityClass,
    /// World origin of the item's spawn pad.
    pub origin: Vec3,
    /// Nearest nav-graph node, resolved at build time (`None` if none within range).
    pub nav_node: Option<usize>,
}

/// Per-tick inputs handed to a brain. Bundled into one struct so the downstream behavior plans
/// (26–33) can add fields (observed enemy weapon, damage/sound events, water/air from the
/// playerstate) **without** changing every brain's `tick` signature. Constructed inline at the
/// call site each frame; the borrows end when `tick` returns.
pub struct BrainContext<'a> {
    /// This frame's perceived world (self + PVS entities + configstrings).
    pub view: &'a Worldview,
    /// The injected navigator (used, never owned). `None` before the map loads.
    pub nav: Option<&'a mut dyn Navigator>,
    /// Collision model for LOS / floor traces. `None` before the map loads.
    pub cm: Option<&'a CollisionModel>,
    /// Seconds this frame covers (the usercmd `msec` as a float).
    pub dt: f32,
    /// Monotonic tick counter since connect (drives jitter, roam dwell, periodic jumps).
    pub ticks: u32,
    /// Per-tick goal injection: when `Some`, the brain drives to this goal instead of its own
    /// FSM/item/roam ladder. Resolved lazily by the caller (e.g. the scenario harness picks the
    /// farthest reachable spawn on the first active frame), which a static config knob can't carry.
    pub goal_override: Option<NavGoal>,
}

/// Per-map facts a brain learns once the map has loaded (was `Brain::set_map`'s args).
pub struct BrainMap {
    /// Roam goal cursor — node indices into the A* graph.
    pub roam_nodes: Vec<usize>,
    /// The A* graph handle (lets the navmesh backend resolve a roam node to a world position).
    pub nav_graph: Arc<NavGraph>,
    /// `true` for backends (navmesh) that path to world positions, not bare node indices.
    pub roam_as_position: bool,
    /// Static item spawns known from the map file (Plan 30) — for map-known resource seeking
    /// (health-when-hurt, ammo re-arm) beyond PVS. Empty until wired at a call site.
    pub items: Vec<MapItem>,
}

/// Tunables that select a brain *flavor* without changing the decision code.
///
/// The default reproduces the live fleet bot exactly. The movement-scenario runner overrides
/// both fields (combat off, goal pinned).
#[derive(Debug, Clone)]
pub struct BrainConfig {
    /// When `false`, combat is never evaluated (no target, no fire) — the bot only navigates.
    /// Used by the movement-test scenarios (and `--brain main` A/B pathing runs).
    pub combat_enabled: bool,
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            combat_enabled: true,
        }
    }
}

/// What one brain tick decides, handed to the caller's driver layer.
#[derive(Debug, Clone, Copy)]
pub struct BrainOutput {
    /// The movement intent to encode into a `Usercmd`.
    pub intent: MovementIntent,
    /// A weapon to switch to via `use <name>` this frame, if any.
    pub weapon_request: Option<Weapon>,
    /// Forward-progress intent for the movement recorder's hindered (`H`) flag — the throttled
    /// nav-step forward, which is `0.0` while the bot is deliberately recovering/backing off
    /// (distinct from `intent.forward`, which encodes the actual recovery motion in those
    /// frames). Only the scenario recorder reads it; the live fleet ignores it.
    pub intent_forward: f32,
}

/// The plugin contract every brain implements. `Send` so a bot task can own a `Box<dyn Brain>`.
///
/// Default method bodies let a trivial brain skip the hooks it doesn't need.
pub trait Brain: Send {
    /// Supply per-map facts once the map has loaded.
    fn set_map(&mut self, map: BrainMap);
    /// Decide one frame.
    fn tick(&mut self, ctx: BrainContext) -> BrainOutput;
    /// React to scoring a frag.
    fn on_kill(&mut self) {}
    /// React to dying (reset held-weapon tracking, ease auto-skill, etc).
    fn on_death(&mut self) {}
    /// Danger/popularity heatmap cost weights `(w_danger, w_pop)` for the nav overlay feed.
    fn heatmap_weights(&self) -> (f32, f32) {
        (0.0, 0.0)
    }
    /// Short status label for periodic logging (replaces the main-specific `behavior()` →
    /// `&BehaviorState`; core stays decoupled from any one brain's FSM).
    fn status(&self) -> &str {
        "?"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_combat_on() {
        let cfg = BrainConfig::default();
        assert!(cfg.combat_enabled);
    }
}
