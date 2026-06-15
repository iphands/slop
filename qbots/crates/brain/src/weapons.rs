//! Weapon definitions and selection logic.
//!
//! Q2 weapon enum values match `shared.h:817` (IT_*) indices.
//! Selection scores weapons by ammo availability, distance to target, and power.

/// Weapon indices (from `shared.h:817` — IT_* constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Weapon {
    Blaster = 1,
    Shotgun = 2,
    Nailgun = 3,
    GrenadeLauncher = 4,
    HandGrenade = 5,
    Railgun = 6,
    BFG10k = 7,
    RocketLauncher = 8,
    Hyperblaster = 9,
    Chaingun = 10,
}

impl Weapon {
    /// Projectile speed, or 0 for hitscan weapons.
    pub fn projectile_speed(self) -> Option<f32> {
        match self {
            Self::RocketLauncher => Some(1200.0),
            Self::GrenadeLauncher => Some(750.0),
            Self::HandGrenade => Some(750.0),
            Self::Blaster => Some(500.0),
            Self::Nailgun => Some(2000.0),
            Self::Hyperblaster => Some(1000.0),
            _ => None, // hitscan
        }
    }

    /// True if this is a hitscan weapon (instant hit).
    pub fn is_hitscan(self) -> bool {
        matches!(self, Self::Shotgun | Self::Railgun | Self::Chaingun)
    }

    /// True if this weapon can damage the shooter at close range.
    pub fn self_dangerous(self) -> bool {
        matches!(self, Self::RocketLauncher | Self::GrenadeLauncher)
    }

    /// Minimum safe firing distance for splash weapons.
    pub fn min_safe_distance(self) -> f32 {
        if self.self_dangerous() {
            128.0
        } else {
            0.0
        }
    }

    /// Effective range for scoring. Beyond this, weapon is poor choice.
    pub fn effective_range(self) -> f32 {
        match self {
            Self::Shotgun => 256.0,
            Self::Railgun => 9999.0,
            Self::RocketLauncher => 1500.0,
            Self::GrenadeLauncher => 1200.0,
            Self::BFG10k => 1000.0,
            Self::Chaingun => 800.0,
            Self::Blaster => 600.0,
            Self::Nailgun => 600.0,
            Self::Hyperblaster => 800.0,
            Self::HandGrenade => 1200.0,
        }
    }

    /// Base power score (higher = more damage per shot).
    pub fn power(self) -> f32 {
        match self {
            Self::Railgun => 100.0,
            Self::BFG10k => 90.0,
            Self::RocketLauncher => 70.0,
            Self::GrenadeLauncher => 60.0,
            Self::Shotgun => 50.0,
            Self::Chaingun => 40.0,
            Self::Nailgun => 35.0,
            Self::Hyperblaster => 30.0,
            Self::Blaster => 20.0,
            Self::HandGrenade => 55.0,
        }
    }
}

/// Ammo indices for each weapon (from `shared.h:1135` — STAT_* ammo slots).
impl Weapon {
    pub fn ammo_index(self) -> usize {
        match self {
            Self::Blaster => 0,
            Self::Shotgun => 1,
            Self::Nailgun => 2,
            Self::GrenadeLauncher => 3,
            Self::HandGrenade => 3,
            Self::Railgun => 4,
            Self::BFG10k => 5,
            Self::RocketLauncher => 6,
            Self::Hyperblaster => 7,
            Self::Chaingun => 8,
        }
    }

    /// Ammo cost per shot.
    pub fn ammo_cost(self) -> i32 {
        match self {
            Self::Blaster => 0, // unlimited
            Self::Shotgun => 1,
            Self::Nailgun => 1,
            Self::GrenadeLauncher => 1,
            Self::HandGrenade => 1,
            Self::Railgun => 1,
            Self::BFG10k => 1,
            Self::RocketLauncher => 1,
            Self::Hyperblaster => 1,
            Self::Chaingun => 1,
        }
    }
}

/// Score a weapon for use against a target at `distance`.
/// Returns a score (higher = better choice). 0 means unusable.
pub fn score_weapon(weapon: Weapon, ammo_count: i32, distance: f32) -> f32 {
    // Must have ammo (blaster is unlimited)
    if weapon.ammo_cost() > 0 && ammo_count <= 0 {
        return 0.0;
    }

    // Too close for splash weapons
    if distance < weapon.min_safe_distance() {
        return 0.0;
    }

    // Range falloff: full power within effective range, drops off after
    let range_factor = if distance <= weapon.effective_range() {
        1.0
    } else {
        weapon.effective_range() / distance
    };

    // Close-range bonus for spread weapons (shotgun excels up close)
    let close_bonus = if weapon == Weapon::Shotgun && distance < 128.0 {
        3.0
    } else {
        1.0
    };

    // Ammo scarcity penalty: fewer ammo = lower score (but still usable)
    let ammo_factor = if weapon.ammo_cost() == 0 {
        1.0
    } else if ammo_count <= 1 {
        0.3
    } else {
        1.0
    };

    weapon.power() * range_factor * ammo_factor * close_bonus
}

/// Select the best weapon from available options.
/// `ammo` is the stats array (from PlayerState).
/// Returns the best weapon, or Blaster as fallback.
pub fn select_best_weapon(_current_weapon: Weapon, ammo: &[i32; 32], distance: f32) -> Weapon {
    let all_weapons = [
        Weapon::Blaster,
        Weapon::Shotgun,
        Weapon::Nailgun,
        Weapon::GrenadeLauncher,
        Weapon::HandGrenade,
        Weapon::Railgun,
        Weapon::BFG10k,
        Weapon::RocketLauncher,
        Weapon::Hyperblaster,
        Weapon::Chaingun,
    ];

    let mut best = Weapon::Blaster;
    let mut best_score = 0.0;

    for &w in &all_weapons {
        let idx = w.ammo_index();
        let ammo_count = if idx < ammo.len() { ammo[idx] } else { 0 };
        let s = score_weapon(w, ammo_count, distance);
        if s > best_score {
            best_score = s;
            best = w;
        }
    }

    best
}

/// Check if the bot should switch weapons to fire at the target.
/// Returns true if a better weapon is available than the current one.
pub fn should_switch_weapon(current_weapon: Weapon, ammo: &[i32; 32], distance: f32) -> bool {
    let best = select_best_weapon(current_weapon, ammo, distance);
    best != current_weapon && score_weapon(best, ammo[best.ammo_index()].max(0), distance) > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blaster_always_available() {
        let ammo = [0i32; 32];
        assert!(score_weapon(Weapon::Blaster, ammo[Weapon::Blaster.ammo_index()], 100.0) > 0.0);
    }

    #[test]
    fn no_ammo_means_unusable() {
        let ammo = [0i32; 32];
        assert_eq!(
            score_weapon(Weapon::Shotgun, ammo[Weapon::Shotgun.ammo_index()], 100.0),
            0.0
        );
    }

    #[test]
    fn shotgun_self_dangerous() {
        assert!(!Weapon::Shotgun.self_dangerous());
        assert!(Weapon::RocketLauncher.self_dangerous());
        assert!(!Weapon::Railgun.self_dangerous());
    }

    #[test]
    fn shotgun_too_close() {
        let ammo = [100i32; 32];
        // Shotgun is NOT self-dangerous (pellets aren't splash), so it scores at any distance
        assert!(score_weapon(Weapon::Shotgun, ammo[Weapon::Shotgun.ammo_index()], 50.0) > 0.0);
    }

    #[test]
    fn railgun_best_at_range() {
        let ammo = [100i32; 32];
        let dist = 1000.0;
        let rail_score = score_weapon(Weapon::Railgun, ammo[Weapon::Railgun.ammo_index()], dist);
        let shot_score = score_weapon(Weapon::Shotgun, ammo[Weapon::Shotgun.ammo_index()], dist);
        assert!(
            rail_score > shot_score,
            "railgun should score higher at long range"
        );
    }

    #[test]
    fn shotgun_best_up_close() {
        let ammo = [100i32; 32];
        let dist = 100.0;
        let rail_score = score_weapon(Weapon::Railgun, ammo[Weapon::Railgun.ammo_index()], dist);
        let shot_score = score_weapon(Weapon::Shotgun, ammo[Weapon::Shotgun.ammo_index()], dist);
        assert!(
            shot_score > rail_score,
            "shotgun should score higher at close range"
        );
    }

    #[test]
    fn hitscan_weapons() {
        assert!(Weapon::Railgun.is_hitscan());
        assert!(Weapon::Shotgun.is_hitscan());
        assert!(!Weapon::RocketLauncher.is_hitscan());
    }
}
