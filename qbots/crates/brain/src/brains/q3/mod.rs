//! # brain::brains::q3 — the Quake 3-derived brain (`q3`; Plan 37)
//!
//! `Q3Brain` reproduces Quake 3 Arena's deathmatch decision loop (`ai_dmnet.c` node FSM +
//! `ai_dmq3.c` aggression/aim/fire) on top of qbots' existing `Navigator`/`world`/`steer`/
//! `recover`. It is a **sibling** to [`MainBrain`](super::main::MainBrain) (the Eraser bot), not
//! a fork: the `trait Brain` seam (Plan 23) lets a second decision philosophy run alongside.
//!
//! The distinctive Q3 ideas (vs `MainBrain`'s flat 5-state FSM):
//! - an **explicit node FSM** — `Seek_LTG`/`Seek_NBG`/`Battle_Fight`/`Battle_Chase`/
//!   `Battle_Retreat`/`Battle_NBG` — whose transitions are gated by the **aggression scalar**
//!   ([`crate::q3char::bot_aggression`]); `Battle_Retreat` (disengage when out-gunned) is new.
//! - the **Q3 aim/fire texture** ([`aim`]): per-weapon accuracy, a reaction-time sight gate, a
//!   fire-throttle duty cycle, radial ground-aim, and a self-preservation fire abort.
//! - **Q3 enemy selection**: alertness-scaled detection range + awareness FOV + LOS.
//!
//! Personality comes from [`Q3Character`] (Plan 36). Navigation is **injected** per tick (same
//! as `MainBrain`); the brain never owns the nav graph.

// `aim` (T4/T5) and `move` (T5) submodules are added with the combat model.

use std::sync::Arc;

use glam::Vec3;
use world::NavGraph;

use crate::brains::core::{Brain, BrainContext, BrainMap, BrainOutput};
use crate::move_ctrl::MovementIntent;
use crate::nav::NavGoal;
use crate::q3char::Q3Character;
use crate::recover::{Recovery, RecoveryAction};
use crate::skill::BotSkill;
use crate::steer::{move_from_world_dir, Steering};
use crate::{items, weapons};

/// The Quake 3 deathmatch FSM nodes (`ai_dmnet.h`, DM-relevant subset; distilled §1). The
/// CTF/teamplay/mission nodes (`Seek_ActivateEntity`, `Stand`, `Observer`, …) are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Q3Node {
    /// Roam toward a long-term item goal; no enemy. `AINode_Seek_LTG`.
    SeekLtg,
    /// Grab a transient nearby item, then return. `AINode_Seek_NBG`.
    SeekNbg,
    /// Fight the current enemy. `AINode_Battle_Fight`.
    BattleFight,
    /// Chase an enemy that broke line of sight. `AINode_Battle_Chase`.
    BattleChase,
    /// Back away from a fight we're losing (aggression `<` threshold). `AINode_Battle_Retreat`.
    BattleRetreat,
    /// Grab a transient item mid-fight, then return to the battle node. `AINode_Battle_NBG`.
    BattleNbg,
}

impl Q3Node {
    /// Short label for `Brain::status` / logging.
    fn label(self) -> &'static str {
        match self {
            Q3Node::SeekLtg => "seek-ltg",
            Q3Node::SeekNbg => "seek-nbg",
            Q3Node::BattleFight => "fight",
            Q3Node::BattleChase => "chase",
            Q3Node::BattleRetreat => "retreat",
            Q3Node::BattleNbg => "battle-nbg",
        }
    }
}

/// Max node switches per tick before we clamp + log (`MAX_NODESWITCHES`, distilled §1). A higher
/// count means the FSM is thrashing — a safety net, not normal operation. (Used by T2's FSM.)
#[allow(dead_code)]
const MAX_NODESWITCHES: u32 = 50;

/// The Quake 3-derived decision brain. Owns the node FSM + Q3 character + combat sub-state;
/// the `Navigator` is injected each [`tick`](Brain::tick).
// Several combat sub-state fields are populated as the FSM + aim/fire model land across T2–T5;
// the `dead_code` allow is removed once they are all live (T5).
#[allow(dead_code)]
pub struct Q3Brain {
    /// The personality (Plan 36) — drives aggression bias, aim, alertness, dodge, firethrottle.
    ch: Q3Character,
    /// Current FSM node.
    node: Q3Node,
    /// Node to return to after a `BattleNbg`/`SeekNbg` item grab.
    return_node: Q3Node,

    // ── navigation / roam (mirrors MainBrain) ──────────────────────────────────────────
    roam_nodes: Vec<usize>,
    roam_idx: usize,
    nav_graph: Option<Arc<NavGraph>>,
    roam_as_position: bool,

    // ── steering / recovery (reused primitives) ────────────────────────────────────────
    steering: Steering,
    recovery: Recovery,
    /// Skill used only for the shared item-value model (`items::best_item_goal`).
    item_skill: BotSkill,

    // ── combat sub-state (filled T2–T5) ────────────────────────────────────────────────
    /// The current enemy entity number, if engaged.
    enemy: Option<i32>,
    /// Last position we saw the enemy at (chase/retreat goal).
    last_enemy_pos: Option<Vec3>,
    /// Our health last tick — a drop this frame widens awareness FOV (Q3 §4).
    last_health: i32,
    /// Wall-clock seconds since connect (driven by `dt`); all timers are absolute seconds.
    time: f32,
    /// Deadline (seconds) to give up a chase (`chase_time`, 10 s).
    chase_deadline: f32,
    /// Last time (seconds) the enemy was visible — retreat gives up after 4 s unseen.
    enemy_seen_time: f32,
    /// Next time (seconds) to poll for a nearby-goal pickup (`check_time`, 0.5 s).
    next_nbg_check: f32,
    /// Deadline (seconds) for an in-progress NBG grab (`nbg_time`).
    nbg_deadline: f32,
    /// Seconds the current enemy has been continuously sighted (reaction-time gate). `None`
    /// until first sight.
    enemy_first_seen: Option<f32>,

    // ── fire-throttle duty cycle (T5) ──────────────────────────────────────────────────
    /// End time (seconds) of the current shoot/wait throttle window.
    throttle_until: f32,
    /// Whether the current throttle window permits firing.
    throttle_firing: bool,

    // ── dodge cooldowns (T5) ───────────────────────────────────────────────────────────
    /// Next time (seconds) a dodge-jump is allowed (1 s cooldown).
    next_jump_time: f32,
    /// Next time (seconds) a dodge-crouch is allowed (1 s cooldown).
    next_crouch_time: f32,

    // ── weapon (optimistic held-weapon tracking for `use` requests) ────────────────────
    held_weapon: weapons::Weapon,

    /// Deterministic per-bot jitter seed mixer (aim error / strafe-flip / dodge rolls).
    rng_state: u32,
}

impl Q3Brain {
    /// Construct a Q3 brain with the given character. Nav roam goals + the graph arrive later
    /// via [`set_map`](Brain::set_map). The steering turn-rate scales with the character's aim
    /// skill (a crisp aimer turns faster).
    pub fn new(ch: Q3Character) -> Self {
        let steering = Steering::new(1.0 + ch.aim_skill * 4.0);
        Self {
            ch,
            node: Q3Node::SeekLtg,
            return_node: Q3Node::SeekLtg,
            roam_nodes: Vec::new(),
            roam_idx: 0,
            nav_graph: None,
            roam_as_position: false,
            steering,
            recovery: Recovery::new(),
            item_skill: BotSkill::default(),
            enemy: None,
            last_enemy_pos: None,
            last_health: 100,
            time: 0.0,
            chase_deadline: 0.0,
            enemy_seen_time: 0.0,
            next_nbg_check: 0.0,
            nbg_deadline: 0.0,
            enemy_first_seen: None,
            throttle_until: 0.0,
            throttle_firing: true,
            next_jump_time: 0.0,
            next_crouch_time: 0.0,
            held_weapon: weapons::Weapon::Blaster,
            rng_state: 0x9e3779b9,
        }
    }

    /// A cheap deterministic `[0,1)` roll (per-bot LCG) for the random Q3 cadences (strafe flip,
    /// dodge chance, fire-throttle window) — keeps behavior repeatable in tests. (Used by T5.)
    #[allow(dead_code)]
    fn roll(&mut self) -> f32 {
        self.rng_state = self
            .rng_state
            .wrapping_mul(1664525)
            .wrapping_add(1013904223);
        (self.rng_state >> 8) as f32 / ((1u32 << 24) as f32)
    }

    /// Resolve the roam long-term-goal: the highest-value visible item, else the next roam
    /// waypoint (cycled every ~5 s), else hold position. Mirrors `MainBrain`'s roam ladder.
    fn roam_goal(&mut self, view: &crate::perception::Worldview, ticks: u32, pos: Vec3) -> NavGoal {
        if let Some((item_pos, _)) = items::best_item_goal(view, &self.item_skill) {
            return NavGoal::Position(item_pos);
        }
        if !self.roam_nodes.is_empty() {
            if ticks.is_multiple_of(50) {
                self.roam_idx =
                    (self.roam_idx + self.roam_nodes.len() / 7 + 1) % self.roam_nodes.len();
            }
            let node = self.roam_nodes[self.roam_idx];
            return if self.roam_as_position {
                match &self.nav_graph {
                    Some(g) => NavGoal::Position(Vec3::from(g.node_pos(node))),
                    None => NavGoal::Waypoint(node),
                }
            } else {
                NavGoal::Waypoint(node)
            };
        }
        NavGoal::Position(pos)
    }

    /// Drive the injected navigator to `goal` and produce the steering portion of a
    /// `MovementIntent` (yaw turn + forward/side + arrive throttle + stuck recovery + jump
    /// edges). This is the non-combat locomotion shared by every node. Combat aim/fire is
    /// layered on top by the battle nodes (T4/T5).
    fn locomote(
        &mut self,
        nav: &mut dyn crate::nav_mode::Navigator,
        cm: Option<&world::CollisionModel>,
        pos: Vec3,
        goal: NavGoal,
        dt: f32,
        mv: &mut MovementIntent,
    ) {
        nav.update(pos, None);
        nav.set_goal(goal, pos);
        if let Some(cm) = cm {
            nav.smooth_with_cm(cm, pos);
        }

        // View yaw: steer along the path look-ahead.
        let ideal_yaw = nav
            .pursue_target(pos)
            .filter(|pt| (pt - pos).length_squared() > 1.0)
            .map(|pt| {
                let d = pt - pos;
                d.y.atan2(d.x).to_degrees()
            })
            .unwrap_or(self.steering.view_yaw());
        let view_yaw = self.steering.change_yaw(ideal_yaw, dt);
        mv.look_at(view_yaw, 0.0);

        // World move direction from path look-ahead.
        let world_dir = nav
            .pursue_target(pos)
            .map(|pt| {
                let d = pt - pos;
                Vec3::new(d.x, d.y, 0.0).normalize_or_zero()
            })
            .unwrap_or(Vec3::ZERO);
        let arrive = nav
            .pursue_target(pos)
            .map(|pt| Steering::arrive_scale((pt - pos).length()))
            .unwrap_or(1.0);
        let (fwd, side) = move_from_world_dir(world_dir, view_yaw, true);
        mv.move_forward(fwd * arrive);
        mv.move_side(side * arrive);

        // Stuck recovery (shared with MainBrain; never "engaging" here — combat nodes set their
        // own gates in T5).
        let has_nav_target = nav.pursue_target(pos).is_some();
        let rec = self
            .recovery
            .evaluate(pos, dt, cm, view_yaw, has_nav_target, false);
        match rec {
            RecoveryAction::None => {}
            RecoveryAction::Jump => mv.jump(),
            RecoveryAction::Strafe { dir } => mv.move_side(dir),
            RecoveryAction::BackOffThenRepath => {
                mv.move_forward(-0.5);
                nav.force_replan();
            }
            RecoveryAction::UseHeading(yaw) => {
                let r = yaw.to_radians();
                let free_dir = Vec3::new(r.cos(), r.sin(), 0.0);
                let (hfwd, hside) = move_from_world_dir(free_dir, view_yaw, true);
                mv.move_forward(hfwd);
                mv.move_side(hside);
            }
        }

        if nav.current_edge_is_jump() {
            mv.jump();
        }
    }
}

impl Brain for Q3Brain {
    fn set_map(&mut self, map: BrainMap) {
        let BrainMap {
            roam_nodes,
            nav_graph,
            roam_as_position,
        } = map;
        self.roam_nodes = roam_nodes;
        self.nav_graph = Some(nav_graph);
        self.roam_as_position = roam_as_position;
    }

    fn status(&self) -> &str {
        self.node.label()
    }

    fn on_kill(&mut self) {
        // Reacquire fresh next fight; keep the character fixed (no auto-skill drift in Q3).
        self.enemy = None;
    }

    fn on_death(&mut self) {
        // Respawn loadout is the blaster; reset combat state.
        self.held_weapon = weapons::Weapon::Blaster;
        self.enemy = None;
        self.last_enemy_pos = None;
        self.enemy_first_seen = None;
        self.node = Q3Node::SeekLtg;
        self.return_node = Q3Node::SeekLtg;
    }

    fn tick(&mut self, ctx: BrainContext) -> BrainOutput {
        let BrainContext {
            view,
            nav,
            cm,
            dt,
            ticks,
            goal_override,
        } = ctx;
        self.time += dt;

        let mut mv = MovementIntent::new();
        let pos = view.self_state().origin;

        // T1: roam-only. T2 layers node transitions; T3–T5 enemy select + aim/fire/move.
        if let Some(nav) = nav {
            let goal = goal_override
                .clone()
                .unwrap_or_else(|| self.roam_goal(view, ticks, pos));
            self.locomote(nav, cm, pos, goal, dt, &mut mv);
        } else {
            // No nav graph yet — walk forward so the bot isn't a statue.
            mv.move_forward(1.0);
        }

        self.last_health = view.self_state().health;

        BrainOutput {
            intent: mv,
            weapon_request: None,
            intent_forward: mv.forward,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav_mode::StubNav;
    use crate::perception::Worldview;
    use client::parse::ConfigStrings;
    use q2proto::Frame;

    fn empty_view() -> Worldview {
        Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0)
    }

    #[test]
    fn new_brain_starts_in_seek_ltg() {
        let b = Q3Brain::new(Q3Character::default());
        assert_eq!(b.status(), "seek-ltg");
    }

    #[test]
    fn roam_drives_navigator_to_a_goal() {
        let mut b = Q3Brain::new(Q3Character::default());
        let view = empty_view();
        let mut nav = StubNav {
            pursue: Some(Vec3::new(200.0, 0.0, 0.0)),
            ..Default::default()
        };
        let out = b.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: None,
        });
        // With a look-ahead point ahead in +x, the bot should command some forward motion.
        assert!(nav.last_goal.is_some(), "a roam goal was set");
        assert!(out.intent.forward > 0.0, "bot walks toward the look-ahead");
    }

    #[test]
    fn no_nav_walks_forward() {
        let mut b = Q3Brain::new(Q3Character::default());
        let view = empty_view();
        let out = b.tick(BrainContext {
            view: &view,
            nav: None,
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: None,
        });
        assert_eq!(out.intent.forward, 1.0);
    }

    #[test]
    fn on_death_resets_to_seek_ltg() {
        let mut b = Q3Brain::new(Q3Character::sarge());
        b.node = Q3Node::BattleFight;
        b.enemy = Some(7);
        b.on_death();
        assert_eq!(b.status(), "seek-ltg");
        assert!(b.enemy.is_none());
    }
}
