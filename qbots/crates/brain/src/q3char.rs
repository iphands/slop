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

use crate::perception::Worldview;
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

/// A selectable named Q3 personality (Plan 38 roster). A clap `ValueEnum` so `--q3char <name>`
/// picks one; each maps to a [`Q3Character`] preset and carries a stable [`tag`](Self::tag) for
/// per-character bot names + scoreboard grouping.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Q3CharPreset {
    /// Low skill, high spray, weak aim ([`Q3Character::grunt`]).
    Grunt,
    /// High aim skill, precise, low firethrottle ([`Q3Character::major`]).
    Major,
    /// High aggression + jumper, mobile brawler ([`Q3Character::sarge`]).
    Sarge,
    /// High camper/alertness, low aggression ([`Q3Character::camper`]).
    Camper,
}

impl Q3CharPreset {
    /// The `Q3Character` this preset selects.
    pub fn character(self) -> Q3Character {
        match self {
            Q3CharPreset::Grunt => Q3Character::grunt(),
            Q3CharPreset::Major => Q3Character::major(),
            Q3CharPreset::Sarge => Q3Character::sarge(),
            Q3CharPreset::Camper => Q3Character::camper(),
        }
    }

    /// Stable kebab tag for names / scoreboard grouping.
    pub fn tag(self) -> &'static str {
        match self {
            Q3CharPreset::Grunt => "grunt",
            Q3CharPreset::Major => "major",
            Q3CharPreset::Sarge => "sarge",
            Q3CharPreset::Camper => "camper",
        }
    }

    /// A distinct Q2 player skin (`model/skin`) per character, so the roster is visually
    /// recognizable in-game and on the scoreboard.
    pub fn skin(self) -> &'static str {
        match self {
            Q3CharPreset::Grunt => "male/grunt",
            Q3CharPreset::Major => "male/major",
            Q3CharPreset::Sarge => "male/sarge",
            Q3CharPreset::Camper => "female/athena",
        }
    }
}

/// Does the **held** weapon have enough ammo to count toward aggression? Thresholds mirror
/// the Q3 ladder (`ai_dmq3.c:2199`, distilled §2), read against the only ammo we see —
/// `STAT_AMMO`, the held weapon's count. Weapons that are never a "real" aggression weapon
/// (Blaster / Machinegun / Chaingun) return `false` so they fall through to flee.
fn ammo_sufficient(weapon: Weapon, held_ammo: i32) -> bool {
    match weapon {
        Weapon::Bfg10k => held_ammo > 7,
        Weapon::Railgun => held_ammo > 5,
        Weapon::Hyperblaster => held_ammo > 50, // ~Q3 lightning/plasma (cells)
        Weapon::RocketLauncher => held_ammo > 5,
        Weapon::GrenadeLauncher => held_ammo > 10,
        Weapon::SuperShotgun | Weapon::Shotgun => held_ammo > 10,
        Weapon::Blaster | Weapon::Machinegun | Weapon::Chaingun => false,
    }
}

/// **`BotAggression`** (`ai_dmq3.c:2199`) — a 0–100 scalar computed from the bot's loadout +
/// health + armor (+ optional enemy geometry). Threshold (default 50, character-biased by
/// [`Q3Character::retreat_threshold`]) gates retreat (`<`) and chase (`>`).
///
/// **qbots adaptation (distilled §2).** Stock Q3 scans a full inventory; we read only the
/// **held** weapon ([`SelfState::held_weapon`](crate::perception::SelfState)) and its ammo
/// ([`SelfState::held_ammo`](crate::perception::SelfState)). The held weapon's
/// [`Weapon::power_tier`] *is* its aggression score once the ammo gate passes; weak weapons
/// (tier `<50`: Machinegun/Chaingun/Blaster, or out of ammo) score 0 → flee. The QUAD branch
/// (`return 70`) is **dropped** — the quad timer isn't reliably wire-visible. `BotAggression`
/// is loadout-based and is intentionally **not** scaled by the character (faithful to stock
/// Q3, where AGGRESSION biases the *threshold*, not this scalar) — so this fn takes no
/// `Q3Character`; the bias lives in [`wants_to_retreat`]/[`wants_to_chase`].
///
/// `enemy_height_delta` = `enemy.z − self.z` (world units), `None` if no enemy is in view. A
/// positive delta `> 200` (enemy well above us → bad firing angle) forces aggression to 0,
/// mirroring the Q3 `> 200` height guard.
pub fn bot_aggression(view: &Worldview, enemy_height_delta: Option<f32>) -> f32 {
    let ss = view.self_state();

    // Enemy far above → bad angle, don't press.
    if matches!(enemy_height_delta, Some(dz) if dz > 200.0) {
        return 0.0;
    }
    // Health/armor guards (Q3 returns 0 below these).
    if ss.health < 60 {
        return 0.0;
    }
    if ss.health < 80 && ss.armor < 40 {
        return 0.0;
    }

    let Some(weapon) = ss.held_weapon else {
        return 0.0;
    };
    // Q2 blaster deviation: the blaster is Q2's always-available, infinite-ammo *start* weapon —
    // unlike Q3's melee-only gauntlet (aggression 0 → flee). A *healthy* bot fights with it
    // rather than fleeing forever (without this, q3 bots back out of blaster range and never
    // engage on the Q2 loadout — see Plan 37 T8). It's the weakest engage-worthy weapon → a
    // fixed 50 (== shotgun); real weapons rank above via their ammo-gated tier, so weapon quality
    // still scales how hard the bot *chases* (rail bot at 95 presses far harder than a blaster).
    if weapon == Weapon::Blaster {
        return 50.0;
    }
    if !ammo_sufficient(weapon, ss.held_ammo()) {
        return 0.0;
    }
    let tier = weapon.power_tier();
    // Below the "real weapon" line (SG=50) → flee even with ammo (bare MG/CG).
    if tier < 50 {
        return 0.0;
    }
    tier as f32
}

/// **`BotFeelingBad`** (`ai_dmq3.c:2247`) — a 0–100 "I'm in trouble" scalar used as a
/// secondary retreat trigger. Q3: gauntlet→100, health<40→100, machinegun→90, health<60→80.
/// qbots maps Q2's weakest weapon (Blaster) to the gauntlet branch.
pub fn bot_feeling_bad(view: &Worldview) -> f32 {
    let ss = view.self_state();
    if ss.held_weapon == Some(Weapon::Blaster) {
        return 100.0;
    }
    if ss.health < 40 {
        return 100.0;
    }
    if ss.held_weapon == Some(Weapon::Machinegun) {
        return 90.0;
    }
    if ss.health < 60 {
        return 80.0;
    }
    0.0
}

/// **`BotWantsToRetreat`** (`ai_dmq3.c:2268`) — `bot_aggression < threshold`. The threshold is
/// the character-biased [`Q3Character::retreat_threshold`] (stock Q3 uses a fixed 50).
pub fn wants_to_retreat(
    view: &Worldview,
    ch: &Q3Character,
    enemy_height_delta: Option<f32>,
) -> bool {
    bot_aggression(view, enemy_height_delta) < ch.retreat_threshold()
}

/// **`BotWantsToChase`** (`ai_dmq3.c:2321`) — `bot_aggression > threshold` (character-biased).
pub fn wants_to_chase(view: &Worldview, ch: &Q3Character, enemy_height_delta: Option<f32>) -> bool {
    bot_aggression(view, enemy_height_delta) > ch.retreat_threshold()
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::parse::ConfigStrings;
    use q2proto::Frame;

    /// CS_MODELS — the configstring model table base (mirrors `perception::CS_MODELS`).
    const CS_MODELS: usize = 32;

    /// Build a synthetic single-frame `Worldview` with a chosen held weapon + stats, so the
    /// aggression ladder can be exercised with no server. `gunindex` names the view-model
    /// configstring that [`Weapon::from_view_model`] resolves.
    fn view_with(
        gunindex: i32,
        view_model: &str,
        health: i16,
        armor: i16,
        held_ammo: i16,
    ) -> Worldview {
        let mut frame = Frame::default();
        frame.playerstate.gunindex = gunindex;
        frame.playerstate.stats[1] = health; // STAT_HEALTH
        frame.playerstate.stats[3] = held_ammo; // STAT_AMMO
        frame.playerstate.stats[5] = armor; // STAT_ARMOR
        let mut cs = ConfigStrings::default();
        cs.set(CS_MODELS + gunindex as usize, view_model);
        Worldview::from_frame(&frame, &cs, 0)
    }

    #[test]
    fn railgun_full_health_presses() {
        // Railgun + 8 slugs + full health → tier 95, wants to chase.
        let view = view_with(1, "models/weapons/v_rail/tris.md2", 100, 100, 8);
        assert_eq!(view.self_state().held_weapon, Some(Weapon::Railgun));
        assert_eq!(bot_aggression(&view, None), 95.0);
        let ch = Q3Character::from_skill(5);
        assert!(wants_to_chase(&view, &ch, None));
        assert!(!wants_to_retreat(&view, &ch, None));
    }

    #[test]
    fn machinegun_hurt_flees() {
        // Machinegun + health 50 → health<60 guard → aggression 0 → retreat.
        let view = view_with(1, "models/weapons/v_machn/tris.md2", 50, 0, 200);
        assert_eq!(bot_aggression(&view, None), 0.0);
        let ch = Q3Character::from_skill(5);
        assert!(wants_to_retreat(&view, &ch, None));
        assert!(!wants_to_chase(&view, &ch, None));
    }

    #[test]
    fn shotgun_healthy_is_boundary_50() {
        // Health 90 (≥80, armor unchecked) + shotgun + 20 shells → tier 50 (the boundary).
        let view = view_with(1, "models/weapons/v_shotg/tris.md2", 90, 0, 20);
        assert_eq!(bot_aggression(&view, None), 50.0);
        // Neutral character threshold is exactly 50 → neither retreat (<50) nor chase (>50).
        let ch = Q3Character::from_skill(5);
        assert!(!wants_to_retreat(&view, &ch, None));
        assert!(!wants_to_chase(&view, &ch, None));
    }

    #[test]
    fn railgun_out_of_ammo_flees() {
        // Railgun but only 3 slugs (≤5) → ammo gate fails → aggression 0.
        let view = view_with(1, "models/weapons/v_rail/tris.md2", 100, 100, 3);
        assert_eq!(bot_aggression(&view, None), 0.0);
    }

    #[test]
    fn healthy_blaster_engages_q2_deviation() {
        // The Q2 start blaster floors a healthy bot at 50 (engage-worthy) so q3 bots aren't
        // permanently passive on the spawn loadout (Plan 37 T8).
        let healthy = view_with(1, "models/weapons/v_blast/tris.md2", 100, 0, 0);
        assert_eq!(bot_aggression(&healthy, None), 50.0);
        let ch = Q3Character::from_skill(5);
        assert!(
            !wants_to_retreat(&healthy, &ch, None),
            "healthy blaster fights"
        );
        // But a *hurt* blaster bot still flees (health guard wins).
        let hurt = view_with(1, "models/weapons/v_blast/tris.md2", 50, 0, 0);
        assert_eq!(bot_aggression(&hurt, None), 0.0);
        assert!(wants_to_retreat(&hurt, &ch, None));
    }

    #[test]
    fn moderately_hurt_but_armored_still_presses() {
        // Health 70 (<80) but armor 60 (≥40) → guards pass → railgun tier 95.
        let view = view_with(1, "models/weapons/v_rail/tris.md2", 70, 60, 8);
        assert_eq!(bot_aggression(&view, None), 95.0);
        // Same health but no armor → second guard trips → 0.
        let view2 = view_with(1, "models/weapons/v_rail/tris.md2", 70, 0, 8);
        assert_eq!(bot_aggression(&view2, None), 0.0);
    }

    #[test]
    fn enemy_high_above_is_bad_angle() {
        // Railgun, full health, but enemy 300u above → bad angle → 0.
        let view = view_with(1, "models/weapons/v_rail/tris.md2", 100, 100, 8);
        assert_eq!(bot_aggression(&view, Some(300.0)), 0.0);
        // Enemy level/below → fine.
        assert_eq!(bot_aggression(&view, Some(-50.0)), 95.0);
    }

    #[test]
    fn aggression_threshold_bias_shifts_engage() {
        // Shotgun (tier 50) + healthy. A high-aggression Sarge (threshold <50) chases;
        // a low-aggression Camper (threshold >50) retreats — same loadout, different bias.
        let view = view_with(1, "models/weapons/v_shotg/tris.md2", 100, 100, 20);
        assert_eq!(bot_aggression(&view, None), 50.0);
        assert!(wants_to_chase(&view, &Q3Character::sarge(), None));
        assert!(wants_to_retreat(&view, &Q3Character::camper(), None));
    }

    #[test]
    fn feeling_bad_ladder() {
        // Blaster (gauntlet analog) → 100 regardless of health.
        let blaster = view_with(1, "models/weapons/v_blast/tris.md2", 100, 0, 0);
        assert_eq!(bot_feeling_bad(&blaster), 100.0);
        // Low health → 100.
        let low = view_with(1, "models/weapons/v_rail/tris.md2", 30, 0, 8);
        assert_eq!(bot_feeling_bad(&low), 100.0);
        // Machinegun (healthy enough) → 90.
        let mg = view_with(1, "models/weapons/v_machn/tris.md2", 100, 0, 200);
        assert_eq!(bot_feeling_bad(&mg), 90.0);
        // Railgun, health 55 → health<60 branch → 80.
        let mid = view_with(1, "models/weapons/v_rail/tris.md2", 55, 0, 8);
        assert_eq!(bot_feeling_bad(&mid), 80.0);
        // Railgun, full health → 0.
        let ok = view_with(1, "models/weapons/v_rail/tris.md2", 100, 0, 8);
        assert_eq!(bot_feeling_bad(&ok), 0.0);
    }

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
    fn presets_map_to_characters_and_tags() {
        use clap::ValueEnum;
        assert_eq!(
            Q3CharPreset::from_str("grunt", true),
            Ok(Q3CharPreset::Grunt)
        );
        assert_eq!(
            Q3CharPreset::from_str("major", true),
            Ok(Q3CharPreset::Major)
        );
        assert!(Q3CharPreset::from_str("nope", true).is_err());
        assert_eq!(Q3CharPreset::Sarge.character(), Q3Character::sarge());
        assert_eq!(Q3CharPreset::Camper.tag(), "camper");
        // Each preset has a distinct skin.
        let skins = [
            Q3CharPreset::Grunt.skin(),
            Q3CharPreset::Major.skin(),
            Q3CharPreset::Sarge.skin(),
            Q3CharPreset::Camper.skin(),
        ];
        let unique: std::collections::HashSet<_> = skins.iter().collect();
        assert_eq!(unique.len(), 4, "distinct skins per character");
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
