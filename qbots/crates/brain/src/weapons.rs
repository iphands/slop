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
pub fn select_best_weapon(held: Weapon, distance: f32) -> Weapon {
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
        assert_eq!(select_best_weapon(Weapon::Blaster, 2000.0), Weapon::Railgun);
    }

    #[test]
    fn select_best_keeps_held_when_near_best() {
        // If we already hold the top weapon, no switch is requested.
        assert_eq!(select_best_weapon(Weapon::Railgun, 2000.0), Weapon::Railgun);
    }
}
