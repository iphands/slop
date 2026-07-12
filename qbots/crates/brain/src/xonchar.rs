//! Xonotic havocbot personality — global skill + **12 additive per-behavior skill offsets**
//! (Plan 59 T1; research: `context/distilled/xonotic.md` §7).
//!
//! Xonotic's per-bot personality model is simpler and broader than Q3's trait fuzz or
//! Eraser's `bots.cfg`: one global `skill` plus 12 named offsets loaded from tab-separated
//! `bot_config_file` rows (`READSKILL`, vendor `bot.qc:275-290`), each **added to the global
//! skill at its point of use** — e.g. the aim filter blend reads `skill + bot_aimskill`, the
//! keyboard re-key clock reads `skill + havocbot_keyboardskill`. So a "sniper" is literally
//! "+3 aim skill, −1 aggression" on top of whatever the server skill is.
//!
//! [`XonSkill`]'s accessors mirror those exact sums; the vendor's use-site `bound(...)`
//! clamps stay at the use sites (`xoncore`), as in the original.

use crate::weapons::Weapon;

/// The 12 per-behavior offsets (vendor `bot.qc:275-290` READSKILL order). All default to 0
/// (a bot exactly at the global skill). Vendor rows use roughly `[-3, +3]`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct XonAxes {
    /// `havocbot_keyboardskill` — keyboard re-key rate (how fast key combos are re-decided).
    pub keyboard: f32,
    /// `bot_moveskill` — movement quality (keyboard tier gating, overshoot stops, bunnyhop).
    pub movement: f32,
    /// `bot_dodgeskill` — projectile-dodge scale + danger evade weight.
    pub dodge: f32,
    /// `bot_pingskill` — REDUCES the simulated ping (`bot.qc:104`); higher = lower latency.
    pub ping: f32,
    /// `bot_weaponskill` — weapon-combo threshold scale (`havocbot.qc:1544-1559`).
    pub weapon: f32,
    /// `bot_aggresskill` — fire-timer burst length + hesitation + enemy-goal bias.
    pub aggres: f32,
    /// `bot_rangepreference` — the `2^rangepreference` effective-distance bias
    /// (`havocbot.qc:1564-1565`). **Used standalone, NOT added to skill.**
    pub rangepref: f32,
    /// `bot_aimskill` — anticipation-filter blend + fire-cone scale.
    pub aim: f32,
    /// `bot_offsetskill` — shrinks the periodic bad-aim offset (`aim.qc:197`).
    pub offset: f32,
    /// `bot_mouseskill` — turn-rate curve exponent input (`aim.qc:294`).
    pub mouse: f32,
    /// `bot_thinkskill` — mouse-retarget ("aim think") period (`aim.qc:263`).
    pub think: f32,
    /// `bot_aiskill` — think-interval scale (`bot.qc:75`) + strategy cadence.
    pub ai: f32,
}

/// A Xonotic bot's effective skill: global `skill` (vendor server cvar, `[0,10]`-ish;
/// `>100` = SUPERBOT which we deliberately do not model) + the 12 additive axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XonSkill {
    /// The global skill level (Xonotic server `skill` cvar analog).
    pub skill: f32,
    /// Per-behavior additive offsets.
    pub axes: XonAxes,
}

impl XonSkill {
    /// A neutral bot at `skill` (all axes 0).
    pub fn new(skill: f32) -> Self {
        Self {
            skill,
            axes: XonAxes::default(),
        }
    }

    // ── the vendor's `skill + bot_*skill` sums, one accessor per use-site family ──────

    /// `skill + bot_aimskill` (filter blend `aim.qc:242`, fire cone `aim.qc:373`).
    pub fn aim(&self) -> f32 {
        self.skill + self.axes.aim
    }
    /// `skill + bot_offsetskill` (bad-aim offset magnitude, `aim.qc:197`).
    pub fn offset(&self) -> f32 {
        self.skill + self.axes.offset
    }
    /// `skill + bot_mouseskill` (turn-rate curve, `aim.qc:294`).
    pub fn mouse(&self) -> f32 {
        self.skill + self.axes.mouse
    }
    /// `skill + bot_thinkskill` (mouse-retarget period, `aim.qc:263-264`).
    pub fn think(&self) -> f32 {
        self.skill + self.axes.think
    }
    /// `skill + bot_aggresskill` (fire timer + hesitation, `aim.qc:325-328`).
    pub fn aggres(&self) -> f32 {
        self.skill + self.axes.aggres
    }
    /// `skill + bot_weaponskill` (weapon-combo window, `havocbot.qc:1546`).
    pub fn weapon(&self) -> f32 {
        self.skill + self.axes.weapon
    }
    /// `skill + bot_dodgeskill` (dodge scale `havocbot.qc:1798`, danger evade `:1249`).
    pub fn dodge(&self) -> f32 {
        self.skill + self.axes.dodge
    }
    /// `skill + bot_moveskill` (keyboard tiers `havocbot.qc:277`, overshoot stop `:1130`).
    pub fn movement(&self) -> f32 {
        self.skill + self.axes.movement
    }
    /// `skill + havocbot_keyboardskill` (re-key clock, `havocbot.qc:281-282`).
    pub fn keyboard(&self) -> f32 {
        self.skill + self.axes.keyboard
    }
    /// `skill + bot_aiskill` (think interval, `bot.qc:75`).
    pub fn ai(&self) -> f32 {
        self.skill + self.axes.ai
    }
    /// `skill + bot_pingskill` (simulated-ping reduction, `bot.qc:104`).
    pub fn ping(&self) -> f32 {
        self.skill + self.axes.ping
    }
    /// `bot_rangepreference` — standalone exponent for `2^rangepreference` distance bias
    /// (`havocbot.qc:1564`); the one axis the vendor does NOT add to skill.
    pub fn range_preference(&self) -> f32 {
        self.axes.rangepref
    }
}

impl Default for XonSkill {
    /// Neutral mid bot (skill 5, all axes 0).
    fn default() -> Self {
        Self::new(5.0)
    }
}

/// A selectable named Xonotic personality (Plan 62 roster; the Plan 38 `CharPreset` pattern).
/// Canonical CLI token = the 3-char code used in competition bot names (Q2's 15-char
/// `netname` limit, Plan 54); long name kept as an alias.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum XonCharPreset {
    /// rusher — presses hard: +aggres/+move/+dodge, closer range preference, sloppier aim.
    #[value(name = "rus", alias = "rusher")]
    Rusher,
    /// sharp — the sniper: +aim/+mouse/+think, longer range preference, less aggressive.
    #[value(name = "shp", alias = "sharp")]
    Sharp,
    /// turtle — cautious survivor: +dodge, −move/−aggres, mid range.
    #[value(name = "trt", alias = "turtle")]
    Turtle,
    /// noob — low skill everywhere: clumsy keys, slow mouse, big aim offsets.
    #[value(name = "nob", alias = "noob")]
    Noob,
}

impl XonCharPreset {
    /// The [`XonSkill`] this preset selects (global skill + axis offsets; Plan 62 tunes
    /// these against the K/D aggregator — treat the numbers as starting points).
    pub fn skill(self) -> XonSkill {
        match self {
            XonCharPreset::Rusher => XonSkill {
                skill: 5.0,
                axes: XonAxes {
                    aggres: 3.0,
                    movement: 2.0,
                    dodge: 1.0,
                    aim: -1.0,
                    offset: -1.0,
                    rangepref: -1.0,
                    ..XonAxes::default()
                },
            },
            XonCharPreset::Sharp => XonSkill {
                skill: 6.0,
                axes: XonAxes {
                    aim: 3.0,
                    mouse: 2.0,
                    think: 1.0,
                    rangepref: 1.5,
                    aggres: -1.0,
                    ..XonAxes::default()
                },
            },
            XonCharPreset::Turtle => XonSkill {
                skill: 4.0,
                axes: XonAxes {
                    dodge: 3.0,
                    movement: -1.0,
                    aggres: -2.0,
                    rangepref: 0.5,
                    ..XonAxes::default()
                },
            },
            XonCharPreset::Noob => XonSkill {
                skill: 2.0,
                axes: XonAxes {
                    keyboard: -1.0,
                    aim: -2.0,
                    mouse: -2.0,
                    think: -1.0,
                    offset: -2.0,
                    ..XonAxes::default()
                },
            },
        }
    }

    /// Stable kebab tag for logs / scoreboard grouping.
    pub fn tag(self) -> &'static str {
        match self {
            XonCharPreset::Rusher => "rusher",
            XonCharPreset::Sharp => "sharp",
            XonCharPreset::Turtle => "turtle",
            XonCharPreset::Noob => "noob",
        }
    }

    /// A distinct Q2 player skin per preset (visual roster recognition, Plan 38 pattern).
    pub fn skin(self) -> &'static str {
        match self {
            XonCharPreset::Rusher => "male/viper",
            XonCharPreset::Sharp => "male/sniper",
            XonCharPreset::Turtle => "male/pmarine",
            XonCharPreset::Noob => "female/jezebel",
        }
    }
}

/// Q2-adapted `bot_pickupbasevalue` table (vendor: Xonotic weapon defs; vortex/devastator
/// 8000, mortar 7000, …, health/armor 5000, ammo 1000–2000). Blaster is the start weapon →
/// 0, matching how Xonotic never rates its laser.
pub fn weapon_base_value(w: Weapon) -> f32 {
    match w {
        Weapon::Blaster => 0.0,
        Weapon::Shotgun => 3000.0,
        Weapon::SuperShotgun => 5000.0,
        Weapon::Machinegun => 4000.0,
        Weapon::Chaingun => 6000.0,
        Weapon::GrenadeLauncher => 7000.0,
        Weapon::RocketLauncher => 8000.0,
        Weapon::Hyperblaster => 6000.0,
        Weapon::Railgun => 8000.0,
        Weapon::Bfg10k => 9000.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_are_skill_plus_axis() {
        let s = XonSkill {
            skill: 5.0,
            axes: XonAxes {
                aim: 2.0,
                mouse: -1.5,
                rangepref: 1.0,
                ..XonAxes::default()
            },
        };
        assert_eq!(s.aim(), 7.0);
        assert_eq!(s.mouse(), 3.5);
        assert_eq!(s.think(), 5.0); // untouched axis = global skill
                                    // rangepref is standalone (never summed with skill) — vendor havocbot.qc:1564.
        assert_eq!(s.range_preference(), 1.0);
    }

    #[test]
    fn presets_are_distinct_and_tagged() {
        let all = [
            XonCharPreset::Rusher,
            XonCharPreset::Sharp,
            XonCharPreset::Turtle,
            XonCharPreset::Noob,
        ];
        for (i, a) in all.iter().enumerate() {
            assert!(a.tag().len() >= 4);
            assert!(!a.skin().is_empty());
            for b in &all[i + 1..] {
                assert_ne!(a.skill(), b.skill(), "{:?} vs {:?}", a, b);
                assert_ne!(a.skin(), b.skin());
            }
        }
    }

    #[test]
    fn preset_flavor_pins() {
        // The roster's defining contrasts (Plan 62 tunes magnitudes, not signs).
        assert!(XonCharPreset::Rusher.skill().aggres() > XonCharPreset::Turtle.skill().aggres());
        assert!(XonCharPreset::Sharp.skill().aim() > XonCharPreset::Noob.skill().aim());
        assert!(XonCharPreset::Sharp.skill().range_preference() > 0.0);
        assert!(XonCharPreset::Rusher.skill().range_preference() < 0.0);
    }

    #[test]
    fn weapon_values_rank_sanely() {
        assert_eq!(weapon_base_value(Weapon::Blaster), 0.0);
        assert!(weapon_base_value(Weapon::Railgun) > weapon_base_value(Weapon::Shotgun));
        assert!(weapon_base_value(Weapon::Bfg10k) >= weapon_base_value(Weapon::RocketLauncher));
    }
}
