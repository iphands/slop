//! Weapon definitions and selection logic.
//!
//! Q2 weapons and their `use <name>` stringcmd names (the only way to switch —
//! the game DLL ignores `usercmd.impulse`; see `g_cmds.c:1945` `Cmd_Use_f`).
//! Names match the binds in `baseq2/config.cfg`.
//!
//! Selection scores weapons by distance to target and power. An external client
//! cannot see its own inventory over the wire (Q2's HUD is server-driven), so
//! ownership is tracked optimistically: we request `use <name>` and the server
//! grants it only if we own the weapon.

/// Q2 weapons. Discriminant values are arbitrary (NOT impulse numbers); they
/// only need a stable ordering. Switching is done via [`Weapon::name`] stringcmds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weapon {
    Blaster,
    Shotgun,
    SuperShotgun,
    Machinegun,
    Chaingun,
    GrenadeLauncher,
    RocketLauncher,
    Hyperblaster,
    Railgun,
    Bfg10k,
}

/// All weapons, strongest first, for iteration.
pub const ALL_WEAPONS: [Weapon; 10] = [
    Weapon::Railgun,
    Weapon::Bfg10k,
    Weapon::RocketLauncher,
    Weapon::GrenadeLauncher,
    Weapon::Hyperblaster,
    Weapon::Chaingun,
    Weapon::Machinegun,
    Weapon::SuperShotgun,
    Weapon::Shotgun,
    Weapon::Blaster,
];

impl Weapon {
    /// `use <name>` stringcmd name — what the server's `Cmd_Use_f` matches against
    /// `item->pickup_name` (`baseq2/config.cfg` binds).
    pub fn name(self) -> &'static str {
        match self {
            Self::Blaster => "Blaster",
            Self::Shotgun => "Shotgun",
            Self::SuperShotgun => "Super Shotgun",
            Self::Machinegun => "Machinegun",
            Self::Chaingun => "Chaingun",
            Self::GrenadeLauncher => "Grenade Launcher",
            Self::RocketLauncher => "Rocket Launcher",
            Self::Hyperblaster => "Hyperblaster",
            Self::Railgun => "Railgun",
            Self::Bfg10k => "BFG10K",
        }
    }

    /// Projectile speed in world units/sec, or `None` for hitscan weapons.
    /// Sources: `fire_blaster` speed=1000, `fire_rocket` speed=650,
    /// grenade 400–800 (default hold ~600), hyperblaster fires blaster bolts.
    pub fn projectile_speed(self) -> Option<f32> {
        match self {
            Self::Blaster => Some(1000.0),
            Self::GrenadeLauncher => Some(600.0),
            Self::RocketLauncher => Some(650.0),
            Self::Hyperblaster => Some(1000.0),
            Self::Bfg10k => Some(800.0),
            _ => None, // hitscan
        }
    }

    /// True for hitscan (instant-hit) weapons.
    pub fn is_hitscan(self) -> bool {
        self.projectile_speed().is_none()
    }

    /// True if this weapon can damage the shooter at close range (splash).
    pub fn self_dangerous(self) -> bool {
        matches!(
            self,
            Self::RocketLauncher | Self::GrenadeLauncher | Self::Bfg10k
        )
    }

    /// Minimum safe firing distance for splash weapons.
    pub fn min_safe_distance(self) -> f32 {
        if self.self_dangerous() {
            match self {
                Self::Bfg10k => 512.0, // huge blast radius
                Self::RocketLauncher => 128.0,
                Self::GrenadeLauncher => 200.0,
                _ => 128.0,
            }
        } else {
            0.0
        }
    }

    /// Effective range for scoring. Beyond this, the weapon is a poor choice.
    pub fn effective_range(self) -> f32 {
        match self {
            Self::Railgun => 4096.0,
            Self::Bfg10k => 2048.0,
            Self::RocketLauncher => 1500.0,
            Self::GrenadeLauncher => 1000.0,
            Self::Hyperblaster => 900.0,
            Self::Chaingun => 800.0,
            Self::Machinegun => 600.0,
            Self::Blaster => 600.0,
            Self::SuperShotgun => 256.0,
            Self::Shotgun => 256.0,
        }
    }

    /// Base power score (higher = more damage per shot).
    pub fn power(self) -> f32 {
        match self {
            Self::Railgun => 100.0,
            Self::Bfg10k => 95.0,
            Self::RocketLauncher => 70.0,
            Self::GrenadeLauncher => 60.0,
            Self::Hyperblaster => 60.0,
            Self::Chaingun => 55.0,
            Self::SuperShotgun => 65.0, // devastating up close
            Self::Machinegun => 40.0,
            Self::Shotgun => 45.0,
            Self::Blaster => 20.0,
        }
    }

    /// Quake 3 aggression tier for this weapon — the 0–100 score the **held**
    /// weapon contributes to [`crate::q3char::bot_aggression`] (`ai_dmq3.c:2199`,
    /// `BotAggression`). Q3 scans a full inventory for the *best* owned weapon; on
    /// the Q2 wire we only see the held weapon, which (Q2 auto-switches to best on
    /// pickup) is a decent proxy. The Q3 tier ladder mapped onto Q2 weapons
    /// (distilled `quake3.md` §2): BFG=100, Railgun=95, Hyperblaster=90,
    /// RocketLauncher=90, GrenadeLauncher=80, Super/Shotgun=50, Machine/Chaingun=25,
    /// Blaster=0. The caller still gates each tier on the held weapon's ammo.
    pub fn power_tier(self) -> u8 {
        match self {
            Self::Bfg10k => 100,
            Self::Railgun => 95,
            Self::Hyperblaster => 90,
            Self::RocketLauncher => 90,
            Self::GrenadeLauncher => 80,
            Self::SuperShotgun => 50,
            Self::Shotgun => 50,
            Self::Machinegun => 25,
            Self::Chaingun => 25,
            Self::Blaster => 0,
        }
    }

    /// Resolve a Q2 **view-weapon model** path (e.g. `models/weapons/v_rail/tris.md2`,
    /// the `gunindex` configstring) to a [`Weapon`]. This is how an external client
    /// learns which weapon it is *holding* — the playerstate carries only `gunindex`
    /// (a CS_MODELS index), and the view-model name disambiguates it. Mapping from
    /// `g_items.c` precache (`v_blast/v_shotg/v_shotg2/v_machn/v_chain/v_launch/
    /// v_rocket/v_hyperb/v_rail/v_bfg`). Returns `None` for non-weapon models.
    pub fn from_view_model(model_str: &str) -> Option<Self> {
        let s = model_str.to_ascii_lowercase();
        // Order matters: check `v_shotg2` (SSG) before `v_shotg` (SG).
        if s.contains("v_shotg2") {
            Some(Self::SuperShotgun)
        } else if s.contains("v_shotg") {
            Some(Self::Shotgun)
        } else if s.contains("v_blast") {
            Some(Self::Blaster)
        } else if s.contains("v_machn") {
            Some(Self::Machinegun)
        } else if s.contains("v_chain") {
            Some(Self::Chaingun)
        } else if s.contains("v_launch") {
            Some(Self::GrenadeLauncher)
        } else if s.contains("v_rocket") {
            Some(Self::RocketLauncher)
        } else if s.contains("v_hyperb") {
            Some(Self::Hyperblaster)
        } else if s.contains("v_rail") {
            Some(Self::Railgun)
        } else if s.contains("v_bfg") {
            Some(Self::Bfg10k)
        } else {
            None
        }
    }

    /// Resolve a Q2 **VWep wield-model** path to the weapon an ENEMY player is holding (Plan 28).
    /// With VWep (stock in Q2 3.20+, always on in yquake2), each player entity carries
    /// `modelindex2` = the third-person wield model, precached in `SP_worldspawn` as `#w_*.md2`
    /// (`vendor/yquake2/src/game/g_spawn.c:762-772`): `#w_blaster/#w_shotgun/#w_sshotgun/
    /// #w_machinegun/#w_chaingun/#w_glauncher/#w_rlauncher/#w_hyperblaster/#w_railgun/#w_bfg`.
    /// Resolving `modelindex2` through CS_MODELS gives us the enemy's weapon — the same trick as
    /// our own `gunindex`→[`from_view_model`](Self::from_view_model). Returns `None` for a
    /// non-weapon / empty model (VWep off, or the enemy holds nothing) — we NEVER guess.
    ///
    /// Note the wield-model names differ from the view models: super-shotgun is `sshotgun`
    /// (checked before `shotgun`), hyperblaster is `hyperblaster` (before `blaster`), the
    /// launchers are `glauncher`/`rlauncher`.
    pub fn from_wield_model(model_str: &str) -> Option<Self> {
        let s = model_str.to_ascii_lowercase();
        if s.contains("sshotgun") {
            Some(Self::SuperShotgun) // before "shotgun"
        } else if s.contains("shotgun") {
            Some(Self::Shotgun)
        } else if s.contains("hyperblaster") {
            Some(Self::Hyperblaster) // before "blaster"
        } else if s.contains("blaster") {
            Some(Self::Blaster)
        } else if s.contains("machinegun") {
            Some(Self::Machinegun)
        } else if s.contains("chaingun") {
            Some(Self::Chaingun)
        } else if s.contains("glauncher") {
            Some(Self::GrenadeLauncher)
        } else if s.contains("rlauncher") {
            Some(Self::RocketLauncher)
        } else if s.contains("railgun") {
            Some(Self::Railgun)
        } else if s.contains("bfg") {
            Some(Self::Bfg10k)
        } else {
            None
        }
    }

    /// Minimum seconds between shots (Eraser `fire_interval`, `bot_wpns.c`).
    /// `0.0` = every frame (chain-/machine-/hyper-blaster). Source: distilled
    /// `eraser.md` §5 fire-interval table.
    pub fn fire_interval_secs(self) -> f32 {
        match self {
            Self::Blaster => 0.6,
            Self::Shotgun | Self::SuperShotgun => 1.0,
            Self::Machinegun | Self::Chaingun | Self::Hyperblaster => 0.0,
            Self::GrenadeLauncher => 0.9,
            Self::RocketLauncher => 0.8,
            Self::Railgun => 1.5,
            Self::Bfg10k => 2.8,
        }
    }
}

/// The distance band a bot should hold from its enemy given the weapon it is **holding** (Plan
/// 28 T2). Replaces `main`'s one-size-fits-all `IDEAL_DIST`/`BACKUP_DIST`: a shotgunner rushes in,
/// a railgunner holds way out, a rocketeer stays outside its own splash. Purely a function of OUR
/// weapon (always known via `gunindex`) — no enemy-weapon read required (which the wire lacks here).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeBand {
    /// Below this distance, back away (too close for this weapon, or inside splash-danger).
    /// Always ≥ the weapon's `min_safe_distance` so we never retreat *into* our own blast.
    pub backup: f32,
    /// The top of the sweet spot: at/above `backup` and below `ideal`, hold + circle-strafe;
    /// above `ideal`, close the distance.
    pub ideal: f32,
}

/// The ideal engagement band for the weapon we are holding (Plan 28 T2). Tuned to each weapon's
/// `effective_range`/`min_safe_distance`: shotguns hug, hitscan-rapid sit mid, splash holds outside
/// its blast, rail/BFG hold long.
pub fn ideal_range(weapon: Weapon) -> RangeBand {
    match weapon {
        // Shotguns: get in their face — power falls off hard past a couple hundred units.
        Weapon::SuperShotgun | Weapon::Shotgun => RangeBand {
            backup: 32.0,
            ideal: 128.0,
        },
        // Rapid hitscan/rapid-projectile: mid-range trading.
        Weapon::Machinegun | Weapon::Chaingun | Weapon::Hyperblaster => RangeBand {
            backup: 96.0,
            ideal: 320.0,
        },
        // Blaster: weak slow bolt — close enough to land it, but no melee hug.
        Weapon::Blaster => RangeBand {
            backup: 80.0,
            ideal: 300.0,
        },
        // Splash: NEVER inside `min_safe_distance` (128) — hold mid-far.
        Weapon::RocketLauncher => RangeBand {
            backup: 160.0,
            ideal: 500.0,
        },
        // Grenades arc + big self-blast (min_safe 200) — hold further.
        Weapon::GrenadeLauncher => RangeBand {
            backup: 220.0,
            ideal: 450.0,
        },
        // BFG: huge blast (min_safe 512) — hold very long.
        Weapon::Bfg10k => RangeBand {
            backup: 560.0,
            ideal: 900.0,
        },
        // Railgun: the longer the better — hold way out, retreat if they close.
        Weapon::Railgun => RangeBand {
            backup: 300.0,
            ideal: 700.0,
        },
    }
}

/// Score a weapon for use against a target at `distance`. Higher is better.
/// Returns 0 if unusable at this distance (e.g. splash weapon too close).
pub fn score_weapon(weapon: Weapon, distance: f32) -> f32 {
    // Too close for splash weapons — would damage ourselves.
    if distance < weapon.min_safe_distance() {
        return 0.0;
    }

    // Range falloff: full power within effective range, drops off after.
    let range_factor = if distance <= weapon.effective_range() {
        1.0
    } else {
        (weapon.effective_range() / distance).max(0.0)
    };

    // Close-range bonus for spread weapons (shotguns excel up close).
    let close_bonus =
        if matches!(weapon, Weapon::Shotgun | Weapon::SuperShotgun) && distance < 128.0 {
            2.0
        } else {
            1.0
        };

    weapon.power() * range_factor * close_bonus
}

/// Select the best weapon for a target at `distance`. Ownership is not known
/// over the wire, so this returns the *desired* weapon; the caller requests it
/// via `use <name>` and the server grants it if owned. Falls back to Blaster
/// (always owned, unlimited ammo) so the bot can always shoot.
///
/// `held_ammo` is the **held** weapon's ammo (`STAT_AMMO`, the only inventory on the wire, Plan 30
/// T4): a dry held weapon (0 ammo, not the Blaster) is treated as **unusable** — it is excluded
/// from the search and never "kept", so the bot stops clicking an empty gun and falls back to the
/// best other weapon (ultimately the Blaster). Pass `i32::MAX` to opt out (no ammo gating).
pub fn select_best_weapon(held: Weapon, distance: f32, held_ammo: i32) -> Weapon {
    // A dry held weapon can't fire → fall back to the Blaster, the ONLY weapon guaranteed owned +
    // loaded. We can't see other weapons' ammo on the wire, and `use <unowned/dry>` is a server
    // no-op that would leave us clicking the empty gun — so the Blaster is the reliable fallback
    // (the item-seeking layer, Plan 30 T3, re-arms us). Blaster itself is never "dry".
    if held != Weapon::Blaster && held_ammo <= 0 {
        return Weapon::Blaster;
    }

    let mut best = Weapon::Blaster;
    let mut best_score = score_weapon(Weapon::Blaster, distance);
    for &w in &ALL_WEAPONS {
        let s = score_weapon(w, distance);
        if s > best_score {
            best_score = s;
            best = w;
        }
    }

    // Don't bother switching if the held weapon is already (near) best.
    if score_weapon(held, distance) >= best_score * 0.95 {
        held
    } else {
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blaster_always_usable() {
        assert!(score_weapon(Weapon::Blaster, 100.0) > 0.0);
    }

    #[test]
    fn splash_too_close_is_zero() {
        // Rocket min safe = 128; firing from 50 would self-damage.
        assert_eq!(score_weapon(Weapon::RocketLauncher, 50.0), 0.0);
        assert!(score_weapon(Weapon::RocketLauncher, 300.0) > 0.0);
    }

    #[test]
    fn railgun_best_at_range() {
        let dist = 2000.0;
        assert!(
            score_weapon(Weapon::Railgun, dist) > score_weapon(Weapon::Shotgun, dist),
            "railgun should score higher at long range"
        );
    }

    #[test]
    fn super_shotgun_best_up_close() {
        let dist = 80.0;
        assert!(
            score_weapon(Weapon::SuperShotgun, dist) > score_weapon(Weapon::Railgun, dist),
            "SSG should out-score railgun point-blank"
        );
    }

    #[test]
    fn names_match_use_stringcmd() {
        assert_eq!(Weapon::RocketLauncher.name(), "Rocket Launcher");
        assert_eq!(Weapon::Bfg10k.name(), "BFG10K");
        assert_eq!(Weapon::SuperShotgun.name(), "Super Shotgun");
    }

    #[test]
    fn hitscan_classification() {
        assert!(Weapon::Railgun.is_hitscan());
        assert!(Weapon::Shotgun.is_hitscan());
        assert!(!Weapon::RocketLauncher.is_hitscan());
        assert!(!Weapon::Blaster.is_hitscan());
    }

    #[test]
    fn select_best_prefers_railgun_at_range() {
        // At long range the Railgun dominates; a Blaster-holder should switch.
        assert_eq!(
            select_best_weapon(Weapon::Blaster, 2000.0, 50),
            Weapon::Railgun
        );
    }

    #[test]
    fn select_best_keeps_held_when_near_best() {
        // If we already hold the top weapon (with ammo), no switch is requested.
        assert_eq!(
            select_best_weapon(Weapon::Railgun, 2000.0, 50),
            Weapon::Railgun
        );
    }

    #[test]
    fn dry_held_weapon_forces_switch_off() {
        // A dry Railgun at range is unusable → don't keep clicking it; switch away (Plan 30 T4).
        let picked = select_best_weapon(Weapon::Railgun, 2000.0, 0);
        assert_ne!(picked, Weapon::Railgun, "dry railgun must not be kept");
        // With no other ammo known, the guaranteed fallback is the Blaster.
        assert_eq!(picked, Weapon::Blaster);
        // The Blaster itself is never "dry" (unlimited) — ammo=0 must NOT change a Blaster-holder's
        // pick (it still upgrades to the best weapon for the range, here the SSG up close).
        assert_eq!(
            select_best_weapon(Weapon::Blaster, 100.0, 0),
            select_best_weapon(Weapon::Blaster, 100.0, 50),
        );
    }

    #[test]
    fn power_tier_ranks_q3_aggression_ladder() {
        // BFG > Railgun > {RL,HB} > GL > {SSG,SG} > {MG,CG} > Blaster (distilled §2).
        assert_eq!(Weapon::Bfg10k.power_tier(), 100);
        assert_eq!(Weapon::Railgun.power_tier(), 95);
        assert!(Weapon::Bfg10k.power_tier() > Weapon::Railgun.power_tier());
        assert!(Weapon::Railgun.power_tier() > Weapon::RocketLauncher.power_tier());
        assert_eq!(
            Weapon::RocketLauncher.power_tier(),
            Weapon::Hyperblaster.power_tier()
        );
        assert!(Weapon::RocketLauncher.power_tier() > Weapon::GrenadeLauncher.power_tier());
        assert!(Weapon::GrenadeLauncher.power_tier() > Weapon::SuperShotgun.power_tier());
        assert_eq!(
            Weapon::SuperShotgun.power_tier(),
            Weapon::Shotgun.power_tier()
        );
        assert!(Weapon::Shotgun.power_tier() > Weapon::Machinegun.power_tier());
        assert_eq!(
            Weapon::Machinegun.power_tier(),
            Weapon::Chaingun.power_tier()
        );
        assert!(Weapon::Machinegun.power_tier() > Weapon::Blaster.power_tier());
        assert_eq!(Weapon::Blaster.power_tier(), 0);
    }

    #[test]
    fn from_view_model_resolves_held_weapon() {
        assert_eq!(
            Weapon::from_view_model("models/weapons/v_rail/tris.md2"),
            Some(Weapon::Railgun)
        );
        // SSG must win over SG (substring `v_shotg` is in both `v_shotg` and `v_shotg2`).
        assert_eq!(
            Weapon::from_view_model("models/weapons/v_shotg2/tris.md2"),
            Some(Weapon::SuperShotgun)
        );
        assert_eq!(
            Weapon::from_view_model("models/weapons/v_shotg/tris.md2"),
            Some(Weapon::Shotgun)
        );
        assert_eq!(
            Weapon::from_view_model("models/weapons/v_launch/tris.md2"),
            Some(Weapon::GrenadeLauncher)
        );
        assert_eq!(
            Weapon::from_view_model("models/weapons/v_bfg/tris.md2"),
            Some(Weapon::Bfg10k)
        );
        assert_eq!(Weapon::from_view_model("players/male/tris.md2"), None);
        assert_eq!(Weapon::from_view_model(""), None);
    }

    #[test]
    fn ideal_range_matches_weapon_character() {
        // Shotgun rushes in; rail holds way out.
        assert!(ideal_range(Weapon::SuperShotgun).ideal < ideal_range(Weapon::Railgun).ideal);
        assert!(ideal_range(Weapon::Shotgun).backup < 64.0, "shotguns hug");
        assert!(
            ideal_range(Weapon::Railgun).backup > 200.0,
            "rail keeps distance"
        );
        // A splash weapon must NEVER back up into its own blast: backup ≥ min_safe_distance.
        for w in [
            Weapon::RocketLauncher,
            Weapon::GrenadeLauncher,
            Weapon::Bfg10k,
        ] {
            assert!(
                ideal_range(w).backup >= w.min_safe_distance(),
                "{w:?}: backup {} must be ≥ min_safe {}",
                ideal_range(w).backup,
                w.min_safe_distance()
            );
        }
        // Every band is coherent: backup < ideal.
        for w in ALL_WEAPONS {
            let b = ideal_range(w);
            assert!(
                b.backup < b.ideal,
                "{w:?}: backup {} !< ideal {}",
                b.backup,
                b.ideal
            );
        }
    }

    #[test]
    fn from_wield_model_resolves_enemy_weapon() {
        // Exact VWep precache names (g_spawn.c:762-772).
        assert_eq!(
            Weapon::from_wield_model("#w_railgun.md2"),
            Some(Weapon::Railgun)
        );
        // The two ordering traps: sshotgun must beat shotgun; hyperblaster must beat blaster.
        assert_eq!(
            Weapon::from_wield_model("#w_sshotgun.md2"),
            Some(Weapon::SuperShotgun)
        );
        assert_eq!(
            Weapon::from_wield_model("#w_shotgun.md2"),
            Some(Weapon::Shotgun)
        );
        assert_eq!(
            Weapon::from_wield_model("#w_hyperblaster.md2"),
            Some(Weapon::Hyperblaster)
        );
        assert_eq!(
            Weapon::from_wield_model("#w_blaster.md2"),
            Some(Weapon::Blaster)
        );
        // The two launchers are distinct tokens.
        assert_eq!(
            Weapon::from_wield_model("#w_glauncher.md2"),
            Some(Weapon::GrenadeLauncher)
        );
        assert_eq!(
            Weapon::from_wield_model("#w_rlauncher.md2"),
            Some(Weapon::RocketLauncher)
        );
        assert_eq!(
            Weapon::from_wield_model("#w_machinegun.md2"),
            Some(Weapon::Machinegun)
        );
        assert_eq!(
            Weapon::from_wield_model("#w_chaingun.md2"),
            Some(Weapon::Chaingun)
        );
        assert_eq!(Weapon::from_wield_model("#w_bfg.md2"), Some(Weapon::Bfg10k));
        // Non-weapon / empty (VWep off, or nothing held) → None, never a guess.
        assert_eq!(Weapon::from_wield_model("players/male/tris.md2"), None);
        assert_eq!(Weapon::from_wield_model(""), None);
    }
}
