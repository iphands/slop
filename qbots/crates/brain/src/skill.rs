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

/// Eraser combat ratings (1.0-5.0). Driven by the master skill level via
/// [`BotSkill::adjust_to_skill`] (Eraser `bot_misc.c:1065`).
///
/// - `accuracy` → aim jitter (`aim.rs`).
/// - `combat`   → reaction delay, FOV, dodge gating.
/// - `aggression` → item-search frequency, chase-abort.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ratings {
    pub accuracy: f32,
    pub aggression: f32,
    pub combat: f32,
}

impl Ratings {
    /// Clamp each rating into Eraser's [1, 5] range.
    fn clamped(self) -> Self {
        Self {
            accuracy: self.accuracy.clamp(1.0, 5.0),
            aggression: self.aggression.clamp(1.0, 5.0),
            combat: self.combat.clamp(1.0, 5.0),
        }
    }
}

impl Default for Ratings {
    fn default() -> Self {
        // Mid-range "balanced" baseline before skill remap.
        Self {
            accuracy: 3.0,
            aggression: 3.0,
            combat: 3.0,
        }
    }
}

/// Per-bot skill/personality parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct BotSkill {
    /// Skill level (0-10). Affects aim accuracy and reaction time.
    pub skill: SkillLevel,
    /// Live auto-skill accumulator (drifts on kill/death; initialized to `skill`).
    pub auto_skill: f32,
    /// Eraser combat ratings (post-`adjust_to_skill`).
    pub ratings: Ratings,
    /// Personality type.
    pub personality: Personality,
    /// Preferred weapon (None = auto-select).
    pub preferred_weapon: Option<u8>,
    /// Reaction time multiplier (1.0 = normal, >1.0 = slower).
    /// Reserved for future use; currently not applied to calculations.
    pub reaction_time: f32,
    /// Eraser `quad_freak`: over-values the Quad Damage item when set.
    pub quad_freak: bool,
    /// Eraser `camper`: dwells at a camp node when set (no pressing enemy/item).
    pub camper: bool,
}

impl BotSkill {
    /// Create a new BotSkill with defaults, then apply the Eraser skill remap
    /// so ratings reflect the starting skill level.
    pub fn new(skill: SkillLevel, personality: Personality) -> Self {
        let mut s = Self {
            skill: skill.clamp(0, 10),
            auto_skill: skill.clamp(0, 10) as f32,
            ratings: Ratings::default(),
            personality,
            preferred_weapon: None,
            reaction_time: 1.0,
            quad_freak: false,
            camper: false,
        };
        s.adjust_to_skill();
        s
    }
}

impl Default for BotSkill {
    fn default() -> Self {
        Self::new(5, Personality::Balanced)
    }
}

impl BotSkill {
    /// Eraser `AdjustRatingsToSkill` (`bot_misc.c:1065`): map the live skill to
    /// the ratings axis (1..5), then `acc/cmb += (s−1)*2.5`, `aggr -= (s−1)*2.0`,
    /// clamped to [1,5]. Higher skill → more accurate/combat-ready, less reckless.
    pub fn adjust_to_skill(&mut self) {
        // Map our 0-10 skill to Eraser's ~1-3 skill axis.
        let s = 1.0 + (self.auto_skill.clamp(0.0, 10.0) / 10.0) * 2.0;
        let delta = (s - 1.0) * 2.5;
        let mut r = self.ratings;
        r.accuracy += delta;
        r.combat += delta;
        r.aggression -= (s - 1.0) * 2.0;
        self.ratings = r.clamped();
    }

    /// `bot_auto_skill` on our own kill (`eraser.md` §skill): bump the live skill
    /// up (+0.2, capped so an already-strong bot doesn't runaway), re-remap ratings.
    pub fn on_kill(&mut self) {
        self.auto_skill = (self.auto_skill + 0.2).min(10.0);
        self.skill = self.auto_skill.round() as u8;
        self.adjust_to_skill();
    }

    /// `bot_auto_skill` on our death: ease the live skill down (−0.2, floor 0).
    pub fn on_death(&mut self) {
        self.auto_skill = (self.auto_skill - 0.2).max(0.0);
        self.skill = self.auto_skill.round() as u8;
        self.adjust_to_skill();
    }

    /// Current accuracy rating (1-5), used by `aim.rs` for jitter.
    pub fn accuracy(&self) -> f32 {
        self.ratings.accuracy
    }

    /// Current combat rating (1-5), used for reaction delay / dodge gating.
    pub fn combat(&self) -> f32 {
        self.ratings.combat
    }

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

    #[test]
    fn adjust_to_skill_raises_accuracy_for_high_skill() {
        let low = BotSkill::new(0, Personality::Balanced);
        let high = BotSkill::new(10, Personality::Balanced);
        assert!(
            high.accuracy() > low.accuracy(),
            "high skill → higher accuracy rating"
        );
        assert!(
            high.combat() > low.combat(),
            "high skill → higher combat rating"
        );
        assert!(
            high.ratings.aggression <= low.ratings.aggression,
            "high skill → lower aggression"
        );
    }

    #[test]
    fn ratings_clamped_to_1_5() {
        let extreme = BotSkill::new(10, Personality::Balanced);
        assert!(extreme.accuracy() <= 5.0);
        assert!(extreme.ratings.aggression >= 1.0);
    }

    #[test]
    fn auto_skill_drifts_on_kill_and_death() {
        let mut s = BotSkill::new(5, Personality::Balanced);
        let start_acc = s.accuracy();
        s.on_kill(); // skill up → accuracy up
        assert!(s.accuracy() >= start_acc);
        s.on_death(); // skill back down
        s.on_death();
        assert!(s.accuracy() <= start_acc + 0.001 || s.auto_skill < 5.0);
    }
}
