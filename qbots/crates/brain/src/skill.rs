//! Per-bot skill and personality configuration.
//!
//! Parameters that scale aim jitter, reaction time, aggressiveness, and weapon
//! preferences. Loaded from config (T7) and applied during combat/FSM ticks.

use std::collections::HashMap;

/// Skill level from 0 (terrible) to 10 (perfect).
pub type SkillLevel = u8;

/// Personality traits that affect bot behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Personality {
    /// Conservative: avoids danger, seeks health early.
    Conservative,
    /// Balanced: standard behavior.
    Balanced,
    /// Aggressive: seeks combat, takes risks.
    Aggressive,
}

/// Per-bot skill/personality parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct BotSkill {
    /// Skill level (0-10). Affects aim accuracy and reaction time.
    pub skill: SkillLevel,
    /// Personality type.
    pub personality: Personality,
    /// Preferred weapon (None = auto-select).
    pub preferred_weapon: Option<u8>,
    /// Reaction time multiplier (1.0 = normal, >1.0 = slower).
    /// Reserved for future use; currently not applied to calculations.
    pub reaction_time: f32,
}

impl BotSkill {
    /// Create a new BotSkill with defaults.
    pub fn new(skill: SkillLevel, personality: Personality) -> Self {
        Self {
            skill: skill.clamp(0, 10),
            personality,
            preferred_weapon: None,
            reaction_time: 1.0,
        }
    }
}

impl Default for BotSkill {
    fn default() -> Self {
        Self::new(5, Personality::Balanced)
    }
}

impl BotSkill {
    /// Aim jitter factor based on skill (0.0-1.0).
    /// Skill 0 = max jitter (1.0), Skill 10 = no jitter (0.0).
    pub fn aim_jitter_factor(&self) -> f32 {
        (10 - self.skill) as f32 / 10.0
    }

    /// Reaction delay in frames based on skill and personality.
    /// Skill 0 = 10 frames, Skill 10 = 0 frames.
    pub fn reaction_delay_frames(&self) -> u32 {
        let base = (10 - self.skill) as u32;
        let personality_mod = match self.personality {
            Personality::Conservative => 2,
            Personality::Balanced => 0,
            Personality::Aggressive => -1,
        };
        (base as i32 + personality_mod).max(0) as u32
    }

    /// Aggressiveness factor (0.0-1.0) for FSM transitions.
    /// Conservative = 0.3, Balanced = 0.5, Aggressive = 0.8.
    pub fn aggressiveness(&self) -> f32 {
        match self.personality {
            Personality::Conservative => 0.3,
            Personality::Balanced => 0.5,
            Personality::Aggressive => 0.8,
        }
    }

    /// Health threshold for fleeing (as percentage).
    /// Conservative = 40%, Balanced = 25%, Aggressive = 15%.
    pub fn flee_health_threshold(&self) -> f32 {
        match self.personality {
            Personality::Conservative => 40.0,
            Personality::Balanced => 25.0,
            Personality::Aggressive => 15.0,
        }
    }

    /// Target switch hesitation (frames).
    /// Higher = less likely to switch targets quickly.
    pub fn target_switch_hesitation(&self) -> u32 {
        let base = 3;
        let skill_bonus = self.skill as u32 / 2;
        base + skill_bonus
    }
}

/// Skill registry for multiple bots.
#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    bots: HashMap<String, BotSkill>,
}

impl SkillRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            bots: HashMap::new(),
        }
    }

    /// Register a bot's skill/personality.
    pub fn register(&mut self, name: String, skill: BotSkill) {
        self.bots.insert(name, skill);
    }

    /// Get a bot's skill by name. Returns default if not found.
    pub fn get(&self, name: &str) -> BotSkill {
        self.bots
            .get(name)
            .cloned()
            .unwrap_or_else(|| BotSkill::new(5, Personality::Balanced))
    }

    /// Remove a bot from the registry.
    pub fn remove(&mut self, name: &str) -> Option<BotSkill> {
        self.bots.remove(name)
    }

    /// Iterate over all registered bots.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &BotSkill)> {
        self.bots.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_clamped_to_range() {
        let skill = BotSkill::new(15, Personality::Balanced);
        assert_eq!(skill.skill, 10);

        let skill = BotSkill::new(0, Personality::Balanced);
        assert_eq!(skill.skill, 0);
    }

    #[test]
    fn aim_jitter_factor_range() {
        let low = BotSkill::new(0, Personality::Balanced);
        assert_eq!(low.aim_jitter_factor(), 1.0);

        let high = BotSkill::new(10, Personality::Balanced);
        assert_eq!(high.aim_jitter_factor(), 0.0);

        let mid = BotSkill::new(5, Personality::Balanced);
        assert_eq!(mid.aim_jitter_factor(), 0.5);
    }

    #[test]
    fn reaction_delay_by_skill() {
        let low = BotSkill::new(0, Personality::Balanced);
        assert_eq!(low.reaction_delay_frames(), 10);

        let high = BotSkill::new(10, Personality::Balanced);
        assert_eq!(high.reaction_delay_frames(), 0);
    }

    #[test]
    fn personality_modifiers() {
        let conservative = BotSkill::new(5, Personality::Conservative);
        assert!(conservative.reaction_delay_frames() > 5);

        let aggressive = BotSkill::new(5, Personality::Aggressive);
        assert!(aggressive.reaction_delay_frames() < 5);
    }

    #[test]
    fn aggressiveness_values() {
        let conservative = BotSkill::new(5, Personality::Conservative);
        assert_eq!(conservative.aggressiveness(), 0.3);

        let balanced = BotSkill::new(5, Personality::Balanced);
        assert_eq!(balanced.aggressiveness(), 0.5);

        let aggressive = BotSkill::new(5, Personality::Aggressive);
        assert_eq!(aggressive.aggressiveness(), 0.8);
    }

    #[test]
    fn flee_threshold_by_personality() {
        let conservative = BotSkill::new(5, Personality::Conservative);
        assert_eq!(conservative.flee_health_threshold(), 40.0);

        let balanced = BotSkill::new(5, Personality::Balanced);
        assert_eq!(balanced.flee_health_threshold(), 25.0);

        let aggressive = BotSkill::new(5, Personality::Aggressive);
        assert_eq!(aggressive.flee_health_threshold(), 15.0);
    }

    #[test]
    fn target_switch_hesitation_increases_with_skill() {
        let low = BotSkill::new(0, Personality::Balanced);
        let high = BotSkill::new(10, Personality::Balanced);
        assert!(high.target_switch_hesitation() >= low.target_switch_hesitation());
    }

    #[test]
    fn registry_operations() {
        let mut registry = SkillRegistry::new();
        let skill = BotSkill::new(7, Personality::Aggressive);
        registry.register("bot1".to_string(), skill);

        assert_eq!(registry.get("bot1").skill, 7);
        assert_eq!(registry.get("bot1").personality, Personality::Aggressive);
        assert_eq!(registry.get("bot2").skill, 5); // default

        registry.remove("bot1");
        assert_eq!(registry.get("bot1").skill, 5); // back to default
    }
}
