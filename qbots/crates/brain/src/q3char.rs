//! # brain::q3char — Quake 3 personality + aggression decision primitives (Plan 36)
//!
//! Pure, server-free port of Quake 3 Arena's bot **personality** ([`Q3Character`], the named
//! `[0,1]` characteristics from `chars.h`/`be_ai_char.c`) and its **decision scalars**
//! ([`bot_aggression`]/[`bot_feeling_bad`], `ai_dmq3.c:2199/2247`). Plan 37's `Q3Brain`
//! assembles these into a node FSM; this module owns only the tested primitives.
//!
//! ## PVS / wire deviation from stock Q3 (important)
//!
//! Stock Q3 reads a full server-side `inventory[]` and computes [`bot_aggression`] from the
//! **best owned** weapon. qbots is an external client: the playerstate gives us only the
//! **held** weapon ([`Worldview::self_state`]'s `held_weapon`, resolved from the `gunindex`
//! view-model) and *its* ammo (Q2 `STAT_AMMO`) — not a free per-weapon inventory. Because Q2
//! auto-switches to the best weapon on pickup, "held" is a reasonable proxy for "best owned".
//! So our [`bot_aggression`] ranks the **held** weapon's [`Weapon::power_tier`], gated by the
//! held weapon's ammo. A fuller observed-inventory (mining pickups/obituaries) is a Plan 38
//! option.
//!
//! ## Coexistence with `BotSkill`
//!
//! [`crate::skill::BotSkill`] is the **Eraser** axis (1–5 accuracy/combat/aggression + 0–10
//! master skill) that the shared `combat.rs`/`aim.rs` and `MainBrain` consume. `Q3Character` is
//! a *different shape* (named `[0,1]` traits, per-weapon accuracy, firethrottle/alertness
//! texture) layered alongside — it does not replace `BotSkill`, so `MainBrain` stays byte-
//! identical and the Q3 brain can reuse the shared combat modules while adding Q3 texture.

use crate::skill::SkillLevel;
use crate::weapons::Weapon;

/// A Quake 3 bot personality — the DM-relevant subset of `chars.h`'s ~48 named
/// characteristics (distilled `quake3.md` §3). All fields are `[0,1]` unless noted; higher =
/// "more of the trait". `reaction_time` is in seconds `[0,5]`. Read via the `bot_*` decision
/// functions and (Plan 37) the Q3 aim/fire/move model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Q3Character {
    /// Combat-movement quality (`chars.h` ATTACK_SKILL #2): `<0.2` stand still, `<0.4` only
    /// close/open the gap, `≥0.4` circle-strafe.
    pub attack_skill: f32,
    /// Delay (seconds, `[0,5]`) before aiming/firing at a just-sighted enemy (REACTIONTIME #6).
    pub reaction_time: f32,
    /// Base aim error magnitude (AIM_ACCURACY #7); `1.0` = perfect.
    pub aim_accuracy: f32,
    /// Enables aim prediction (AIM_SKILL #16): `>0.4` linear lead, `>0.6` radial ground-aim,
    /// `>0.95` "don't aim too early".
    pub aim_skill: f32,
    /// Chance to crouch in combat (CROUCHER #36).
    pub croucher: f32,
    /// Chance to jump in combat / dodge (JUMPER #37).
    pub jumper: f32,
    /// Tendency to walk (slow, quiet) vs run (WALKER #48).
    pub walker: f32,
    /// Engage/disengage bias (AGGRESSION #41). Stock Q3's `BotAggression` is *loadout*-based;
    /// we [PORT] this as a bias on the retreat/chase threshold (see [`Self::retreat_threshold`]).
    pub aggression: f32,
    /// Avoid firing splash weapons near walls / own feet (SELFPRESERVATION #42).
    pub self_preservation: f32,
    /// Tendency to hunt the bot that last killed you (VENGEFULNESS #43).
    pub vengefulness: f32,
    /// Tendency to camp a spot (CAMPER #44).
    pub camper: f32,
    /// `<0.5` won't shoot chatting players; raises target greed (EASY_FRAGGER #45).
    pub easy_fragger: f32,
    /// Enemy detection range + awareness FOV width (ALERTNESS #46).
    pub alertness: f32,
    /// Burst-fire duty cycle (FIRETHROTTLE #47): higher = sprays more / longer trigger holds.
    pub firethrottle: f32,
    /// Optional per-weapon aim accuracy override (`chars.h` #8–15). `None` → use
    /// [`Self::aim_accuracy`] for every weapon. Indexed by [`weapon_index`].
    pub per_weapon_accuracy: Option<[f32; 10]>,
}

/// Stable `[0,10)` index for a weapon into [`Q3Character::per_weapon_accuracy`] (enum order).
pub fn weapon_index(w: Weapon) -> usize {
    match w {
        Weapon::Blaster => 0,
        Weapon::Shotgun => 1,
        Weapon::SuperShotgun => 2,
        Weapon::Machinegun => 3,
        Weapon::Chaingun => 4,
        Weapon::GrenadeLauncher => 5,
        Weapon::RocketLauncher => 6,
        Weapon::Hyperblaster => 7,
        Weapon::Railgun => 8,
        Weapon::Bfg10k => 9,
    }
}

impl Q3Character {
    /// Aim accuracy for a specific weapon: the per-weapon override if present, else the base
    /// [`Self::aim_accuracy`] (mirrors Q3's per-weapon AIM_ACCURACY #8–15 falling back to #7).
    pub fn weapon_accuracy(&self, w: Weapon) -> f32 {
        self.per_weapon_accuracy
            .map(|a| a[weapon_index(w)])
            .unwrap_or(self.aim_accuracy)
    }

    /// Character-biased aggression threshold (distilled §3 note). Stock Q3 uses a fixed `50`
    /// for retreat/chase; we shift it by the AGGRESSION characteristic so a high-aggression
    /// bot presses sooner: `threshold = 50 − (aggression − 0.5)·40`, clamped to `[10, 90]`.
    pub fn retreat_threshold(&self) -> f32 {
        (50.0 - (self.aggression - 0.5) * 40.0).clamp(10.0, 90.0)
    }

    /// Map a master skill level `[0,10]` to a monotonic `Q3Character` (à la Eraser's
    /// `AdjustRatingsToSkill`). Higher skill → higher aim accuracy/skill/attack_skill and
    /// alertness/self-preservation, lower reaction time, lower firethrottle (less spray).
    /// Aggression-flavored traits stay neutral here — the named presets ([`Self::grunt`] …)
    /// set those. Formula (with `s = skill/10` clamped to `[0,1]`):
    /// `aim_accuracy = 0.30 + 0.60·s`, `aim_skill = 0.20 + 0.70·s`,
    /// `attack_skill = 0.30 + 0.60·s`, `reaction_time = 1.20 − 1.00·s`,
    /// `alertness = 0.30 + 0.60·s`, `self_preservation = 0.30 + 0.50·s`,
    /// `firethrottle = 0.70 − 0.50·s`.
    pub fn from_skill(skill: SkillLevel) -> Self {
        let s = (skill.min(10) as f32) / 10.0;
        Self {
            attack_skill: 0.30 + 0.60 * s,
            reaction_time: 1.20 - 1.00 * s,
            aim_accuracy: 0.30 + 0.60 * s,
            aim_skill: 0.20 + 0.70 * s,
            croucher: 0.15,
            jumper: 0.25 + 0.25 * s,
            walker: 0.10,
            aggression: 0.50,
            self_preservation: 0.30 + 0.50 * s,
            vengefulness: 0.40,
            camper: 0.20,
            easy_fragger: 0.50,
            alertness: 0.30 + 0.60 * s,
            firethrottle: 0.70 - 0.50 * s,
            per_weapon_accuracy: None,
        }
    }

    /// **Grunt** — low skill, high firethrottle spray, weak aim. The cannon-fodder bot.
    pub fn grunt() -> Self {
        Self {
            attack_skill: 0.40,
            reaction_time: 0.80,
            aim_accuracy: 0.40,
            aim_skill: 0.30,
            croucher: 0.20,
            jumper: 0.20,
            walker: 0.10,
            aggression: 0.50,
            self_preservation: 0.30,
            vengefulness: 0.50,
            camper: 0.10,
            easy_fragger: 0.60,
            alertness: 0.40,
            firethrottle: 0.70,
            per_weapon_accuracy: None,
        }
    }

    /// **Major** — high aim skill, low firethrottle, precise. The crack shot.
    pub fn major() -> Self {
        Self {
            attack_skill: 0.80,
            reaction_time: 0.30,
            aim_accuracy: 0.90,
            aim_skill: 0.90,
            croucher: 0.10,
            jumper: 0.30,
            walker: 0.10,
            aggression: 0.60,
            self_preservation: 0.70,
            vengefulness: 0.40,
            camper: 0.20,
            easy_fragger: 0.40,
            alertness: 0.80,
            firethrottle: 0.20,
            per_weapon_accuracy: None,
        }
    }

    /// **Sarge** — high aggression + jumper, mobile brawler.
    pub fn sarge() -> Self {
        Self {
            attack_skill: 0.70,
            reaction_time: 0.40,
            aim_accuracy: 0.70,
            aim_skill: 0.60,
            croucher: 0.20,
            jumper: 0.80,
            walker: 0.00,
            aggression: 0.90,
            self_preservation: 0.20,
            vengefulness: 0.70,
            camper: 0.00,
            easy_fragger: 0.70,
            alertness: 0.60,
            firethrottle: 0.40,
            per_weapon_accuracy: None,
        }
    }

    /// **Camper** — high camper/alertness, low aggression, holds spots.
    pub fn camper() -> Self {
        Self {
            attack_skill: 0.60,
            reaction_time: 0.50,
            aim_accuracy: 0.80,
            aim_skill: 0.70,
            croucher: 0.50,
            jumper: 0.10,
            walker: 0.60,
            aggression: 0.20,
            self_preservation: 0.80,
            vengefulness: 0.30,
            camper: 0.90,
            easy_fragger: 0.30,
            alertness: 0.90,
            firethrottle: 0.30,
            per_weapon_accuracy: None,
        }
    }
}

impl Default for Q3Character {
    /// Balanced mid character (≈ [`Self::from_skill(5)`](Self::from_skill)).
    fn default() -> Self {
        Self::from_skill(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_skill_is_monotonic() {
        let lo = Q3Character::from_skill(1);
        let hi = Q3Character::from_skill(9);
        assert!(hi.aim_skill > lo.aim_skill);
        assert!(hi.aim_accuracy > lo.aim_accuracy);
        assert!(hi.attack_skill > lo.attack_skill);
        assert!(hi.reaction_time < lo.reaction_time);
        assert!(hi.firethrottle < lo.firethrottle, "skilled bots spray less");
        assert!(hi.alertness > lo.alertness);
    }

    #[test]
    fn presets_have_intended_spread() {
        assert!(
            Q3Character::grunt().firethrottle > Q3Character::major().firethrottle,
            "grunt sprays more than the precise major"
        );
        assert!(Q3Character::major().aim_skill > Q3Character::grunt().aim_skill);
        assert!(Q3Character::sarge().aggression > Q3Character::camper().aggression);
        assert!(Q3Character::sarge().jumper > Q3Character::camper().jumper);
        assert!(Q3Character::camper().camper > Q3Character::sarge().camper);
    }

    #[test]
    fn retreat_threshold_biases_with_aggression() {
        // High aggression → lower threshold (presses sooner); low aggression → higher.
        assert!(Q3Character::sarge().retreat_threshold() < 50.0);
        assert!(Q3Character::camper().retreat_threshold() > 50.0);
        // Neutral aggression (0.5) → exactly 50.
        let neutral = Q3Character::from_skill(5);
        assert!((neutral.retreat_threshold() - 50.0).abs() < 1e-3);
    }

    #[test]
    fn weapon_accuracy_falls_back_to_base() {
        let ch = Q3Character::major();
        assert_eq!(ch.weapon_accuracy(Weapon::Railgun), ch.aim_accuracy);
        let mut per = [0.5f32; 10];
        per[weapon_index(Weapon::Railgun)] = 0.99;
        let ch2 = Q3Character {
            per_weapon_accuracy: Some(per),
            ..ch
        };
        assert_eq!(ch2.weapon_accuracy(Weapon::Railgun), 0.99);
        assert_eq!(ch2.weapon_accuracy(Weapon::Blaster), 0.5);
    }
}
