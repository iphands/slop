//! # brain::persona — per-bot personality for `main` (Plan 27)
//!
//! Mirrors the [`Q3Character`](crate::q3char::Q3Character) idea (continuous `[0,1]` traits + named
//! presets) but for the `main` brain. Turns `main`'s global tactical `const`s (flee/kite health,
//! kite distance, roam dwell) into **persona-driven** values, so a `main` fleet can read as
//! *different people* — a `rusher` that closes and fights hurt, a `sniper` that holds range and
//! bails early, a `scavenger` that hoards items, a `guard` that camps.
//!
//! **Contract:** [`Persona::default`] / [`Persona::from_bot_skill`] reproduce `main`'s pre-Plan-27
//! constants **exactly** (30 / 50 / 450 / 50-or-250), so converting `main` to read a persona is
//! behavior-preserving for every existing bot; only the opt-in named presets differ.

use crate::skill::BotSkill;
use crate::weapons::Weapon;

/// A `main`-brain personality: continuous `[0,1]` traits the tactical getters derive concrete
/// values from. Additive — later plans (29 chase, 30 items) consume `chase_commit`/`item_greed`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Persona {
    /// Display label (preset name) for logs/scoreboards.
    pub name: &'static str,
    /// How eagerly it presses a fight (high → closes distance, fights on).
    pub aggression: f32,
    /// Tolerance for fighting hurt before disengaging (high → low flee/kite thresholds).
    pub risk_tolerance: f32,
    /// A small weapon bias (never overrides a dominant matchup); `None` = pure auto-select.
    pub weapon_pref: Option<Weapon>,
    /// Camps a node vs roams (long roam dwell when set).
    pub camper: bool,
    /// How doggedly it commits to a chase (Plan 29 consumer). `[0,1]`.
    pub chase_commit: f32,
    /// How much it detours for items (Plan 30 consumer). `[0,1]`.
    pub item_greed: f32,
}

impl Default for Persona {
    /// The behavior-preserving default: `aggression`/`risk_tolerance` = 0.5 so the tactical getters
    /// reproduce `main`'s pre-Plan-27 constants exactly.
    fn default() -> Self {
        Self {
            name: "default",
            aggression: 0.5,
            risk_tolerance: 0.5,
            weapon_pref: None,
            camper: false,
            chase_commit: 0.5,
            item_greed: 0.5,
        }
    }
}

impl Persona {
    /// Derive the persona from a bot's [`BotSkill`], **preserving today's behavior exactly**:
    /// pre-Plan-27 `main` used global tactical consts regardless of skill/personality, so the
    /// traits are the neutral 0.5 defaults; only `camper` carries over (it already drove roam
    /// dwell). Named presets ([`Persona::preset`]) are where real differentiation lives.
    pub fn from_bot_skill(skill: &BotSkill) -> Self {
        Self {
            camper: skill.camper,
            ..Self::default()
        }
    }

    /// Health at/below which `main` hard-disengages (was `FLEE_HEALTH=30`). More risk-tolerant →
    /// fights to a lower health. `risk_tolerance = 0.5 → 30`.
    pub fn flee_health(&self) -> i32 {
        (45.0 - self.risk_tolerance * 30.0).round() as i32
    }

    /// Health at/below which `main` kites rather than presses (was `KITE_HEALTH=50`).
    /// `risk_tolerance = 0.5 → 50`.
    pub fn kite_health(&self) -> i32 {
        (70.0 - self.risk_tolerance * 40.0).round() as i32
    }

    /// Range a kiting `main` opens to before holding (was `KITE_DIST=450`). More aggressive →
    /// stays closer. `aggression = 0.5 → 450`.
    pub fn kite_dist(&self) -> f32 {
        600.0 - self.aggression * 300.0
    }

    /// Ticks a roamer dwells per node (was `camper ? 250 : 50`).
    pub fn roam_dwell(&self) -> u32 {
        if self.camper {
            250
        } else {
            50
        }
    }

    /// Resolve a named preset (`--persona <name>`), or `None` for an unknown name.
    pub fn preset(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "rusher" => Self::rusher(),
            "sniper" => Self::sniper(),
            "scavenger" => Self::scavenger(),
            "guard" => Self::guard(),
            "default" | "balanced" => Self::default(),
            _ => return None,
        })
    }

    /// All selectable preset names (for CLI help / competition matrices).
    pub const PRESET_NAMES: [&'static str; 4] = ["rusher", "sniper", "scavenger", "guard"];

    /// Closes distance, fights hurt, chases kills; shotgun bias.
    pub fn rusher() -> Self {
        Self {
            name: "rusher",
            aggression: 0.9,
            risk_tolerance: 0.8,
            weapon_pref: Some(Weapon::SuperShotgun),
            camper: false,
            chase_commit: 0.9,
            item_greed: 0.4,
        }
    }

    /// Holds range, bails early, patient; railgun bias.
    pub fn sniper() -> Self {
        Self {
            name: "sniper",
            aggression: 0.2,
            risk_tolerance: 0.3,
            weapon_pref: Some(Weapon::Railgun),
            camper: false,
            chase_commit: 0.3,
            item_greed: 0.3,
        }
    }

    /// Hoards items; moderate fighter.
    pub fn scavenger() -> Self {
        Self {
            name: "scavenger",
            aggression: 0.4,
            risk_tolerance: 0.5,
            weapon_pref: None,
            camper: false,
            chase_commit: 0.4,
            item_greed: 0.95,
        }
    }

    /// Camps a spot, holds position, rarely chases.
    pub fn guard() -> Self {
        Self {
            name: "guard",
            aggression: 0.5,
            risk_tolerance: 0.4,
            weapon_pref: None,
            camper: true,
            chase_commit: 0.2,
            item_greed: 0.3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The load-bearing contract (Plan 27 Risk #2): the default persona reproduces `main`'s
    /// pre-Plan-27 constants EXACTLY, so converting `main` to read it changes nothing.
    #[test]
    fn default_reproduces_main_constants() {
        let p = Persona::default();
        assert_eq!(p.flee_health(), 30, "was FLEE_HEALTH");
        assert_eq!(p.kite_health(), 50, "was KITE_HEALTH");
        assert_eq!(p.kite_dist(), 450.0, "was KITE_DIST");
        assert_eq!(p.roam_dwell(), 50, "was the non-camper dwell");
    }

    #[test]
    fn from_bot_skill_preserves_default_and_camper() {
        let p = Persona::from_bot_skill(&BotSkill::default());
        assert_eq!(p, Persona::default());
        // A camper skill carries the long dwell (the one pre-Plan-27 differentiator).
        let s = BotSkill {
            camper: true,
            ..BotSkill::default()
        };
        assert_eq!(Persona::from_bot_skill(&s).roam_dwell(), 250);
    }

    #[test]
    fn presets_differ_on_intended_axes() {
        let rusher = Persona::rusher();
        let sniper = Persona::sniper();
        // Rusher fights hurt + closes; sniper bails early + holds range.
        assert!(rusher.flee_health() < sniper.flee_health());
        assert!(rusher.kite_dist() < sniper.kite_dist());
        // Guard camps; scavenger is greedy for items.
        assert_eq!(Persona::guard().roam_dwell(), 250);
        assert!(Persona::scavenger().item_greed > Persona::default().item_greed);
        // Weapon biases match character.
        assert_eq!(rusher.weapon_pref, Some(Weapon::SuperShotgun));
        assert_eq!(sniper.weapon_pref, Some(Weapon::Railgun));
    }

    #[test]
    fn preset_lookup_is_case_insensitive_and_bounded() {
        assert_eq!(Persona::preset("RUSHER").unwrap().name, "rusher");
        assert!(Persona::preset("nonesuch").is_none());
        // Every advertised preset name resolves.
        for n in Persona::PRESET_NAMES {
            assert!(Persona::preset(n).is_some(), "{n} must resolve");
        }
    }
}
