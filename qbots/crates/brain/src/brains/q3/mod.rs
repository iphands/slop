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

mod aim;
mod r#move;

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
use crate::traverse::{TraversalExecutor, TraversalFrame};
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
/// count means the FSM is thrashing — a safety net, not normal operation.
const MAX_NODESWITCHES: u32 = 50;

/// Is `to_target` (a non-unit direction *from* the viewer) inside a horizontal FOV cone of total
/// angle `fov_deg` centered on the unit `forward`? `fov_deg >= 360` is always true (full
/// awareness). Used for both our awareness cone and the enemy's "is it looking at me?" check.
fn fov_cone(forward: Vec3, to_target: Vec3, fov_deg: f32) -> bool {
    if fov_deg >= 360.0 {
        return true;
    }
    let dir = Vec3::new(to_target.x, to_target.y, 0.0).normalize_or_zero();
    if dir == Vec3::ZERO {
        return true;
    }
    forward.dot(dir) > (fov_deg * 0.5).to_radians().cos()
}

/// The Quake 3-derived decision brain. Owns the node FSM + Q3 character + combat sub-state;
/// the `Navigator` is injected each [`tick`](Brain::tick).
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
    /// The shared ladder/swim/ride executor (Plan 46). `q3` previously had NO traversal — a q3 bot
    /// could not swim, ride a lift/train, or climb a ladder in a live match. Now it delegates like
    /// every other brain (applied in `locomote`, the path-following stage).
    traverse: TraversalExecutor,
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
    /// Time (seconds) of the last weapon switch — gates a 0.1 s mid-change fire lockout.
    weapon_switch_time: f32,

    /// Enemy velocity memory for the Q3 aim direction-change penalty (T4).
    aim: aim::AimState,
    /// Circle-strafe direction + flip timer (T5).
    strafe: r#move::StrafeState,

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
            traverse: TraversalExecutor::new(),
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
            weapon_switch_time: f32::NEG_INFINITY,
            aim: aim::AimState::new(),
            strafe: r#move::StrafeState::new(),
            rng_state: 0x9e3779b9,
        }
    }

    /// A cheap deterministic `[0,1)` roll (per-bot LCG) for the random Q3 cadences (strafe flip,
    /// dodge chance, fire-throttle window) — keeps behavior repeatable in tests.
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
    #[allow(clippy::too_many_arguments)]
    fn locomote(
        &mut self,
        nav: &mut dyn crate::nav_mode::Navigator,
        cm: Option<&world::CollisionModel>,
        pos: Vec3,
        goal: NavGoal,
        dt: f32,
        view: &crate::perception::Worldview,
        mv: &mut MovementIntent,
    ) {
        nav.update(pos, None);
        nav.set_goal(goal, pos);
        if let Some(cm) = cm {
            nav.smooth_with_cm(cm, pos);
        }

        // Corner-cut-safe path look-ahead (Plan 48 L3): hull + lava-aware floor validation,
        // same as MainBrain — the raw look-ahead can cut a corner or a lava pool.
        let pursue_pt = match cm {
            Some(c) => nav.pursue_target_safe(pos, c),
            None => nav.pursue_target(pos),
        };

        // View yaw: steer along the path look-ahead.
        let ideal_yaw = pursue_pt
            .filter(|pt| (pt - pos).length_squared() > 1.0)
            .map(|pt| {
                let d = pt - pos;
                d.y.atan2(d.x).to_degrees()
            })
            .unwrap_or(self.steering.view_yaw());
        let view_yaw = self.steering.change_yaw(ideal_yaw, dt);
        mv.look_at(view_yaw, 0.0);

        // World move direction from path look-ahead.
        let world_dir = pursue_pt
            .map(|pt| {
                let d = pt - pos;
                Vec3::new(d.x, d.y, 0.0).normalize_or_zero()
            })
            .unwrap_or(Vec3::ZERO);
        let arrive = pursue_pt
            .map(|pt| Steering::arrive_scale((pt - pos).length()))
            .unwrap_or(1.0);
        // Creep on hazard-bordered stretches (lava walkways) so 10 Hz tracking error
        // can't step us off the edge (Plan 50).
        let creep = crate::hazard::creep_scale(cm, pos, world_dir);
        let (fwd, side) = move_from_world_dir(world_dir, view_yaw, true);
        mv.move_forward(fwd * arrive * creep);
        mv.move_side(side * arrive * creep);

        // Traversal gates (Plan 46): swim/ride/ladder suspend stuck recovery + jump-edge (a
        // surface bob or a stand-and-wait on a lift is not a wedge). This is where `q3` GAINS all
        // traversal — it previously had none in live matches.
        let gates = self.traverse.gates(nav, cm, pos, dt);

        // Stuck recovery (shared with MainBrain; never "engaging" here — combat nodes set their
        // own gates in T5).
        let has_nav_target = pursue_pt.is_some();
        let rec = if gates.any() {
            RecoveryAction::None
        } else {
            self.recovery
                .evaluate(pos, dt, cm, view_yaw, has_nav_target, false)
        };
        match rec {
            RecoveryAction::None => {}
            RecoveryAction::Jump => mv.jump(),
            RecoveryAction::Strafe { dir } => {
                // Flip a stuck-strafe that would side-step into lava / off a drop (Plan 48 L2).
                mv.move_side(crate::hazard::safe_strafe_dir(cm, pos, view_yaw, dir));
            }
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

        if nav.current_edge_is_jump() && !gates.any() {
            mv.jump();
        }

        // Traversal override (Plan 46): swim (Plan 40) + stateful ride/ladder (Plan 43/35) movement
        // is owned by the shared executor — overwrites the movement axes above on a swim/ride/ladder
        // edge. `fwd`/`side` are the raw (pre-arrive) steering the swim machine reuses.
        let frame = TraversalFrame {
            view,
            cm,
            pos,
            view_yaw,
            steer_fwd: fwd,
            steer_side: side,
            dt,
        };
        self.traverse.apply(mv, gates, nav, &frame);
    }

    // ── enemy / visibility helpers ─────────────────────────────────────────────────────

    /// Is the current enemy entity still present in the worldview at all (even stale)? `false`
    /// once the server stops sending it (dead / left PVS for good).
    fn enemy_alive(&self, view: &crate::perception::Worldview) -> bool {
        self.enemy
            .map(|n| view.entities().any(|e| e.entity_number == n))
            .unwrap_or(false)
    }

    /// Do we have a clear line of sight to the current enemy *this frame*? With no collision
    /// model loaded we optimistically treat a non-stale enemy as visible.
    fn enemy_visible(
        &self,
        view: &crate::perception::Worldview,
        cm: Option<&world::CollisionModel>,
    ) -> bool {
        let Some(n) = self.enemy else { return false };
        let Some(e) = view.entities().find(|x| x.entity_number == n) else {
            return false;
        };
        if e.is_stale {
            return false;
        }
        match cm {
            Some(cm) => {
                let eye = crate::los::eye_origin(view.self_state().origin.into());
                crate::los::has_los_player(cm, eye, e.origin.into())
            }
            None => true,
        }
    }

    /// `enemy.z − self.z` (world units) for the [`crate::q3char`] bad-angle guard. `None` if no
    /// enemy is in view.
    fn enemy_height_delta(&self, view: &crate::perception::Worldview) -> Option<f32> {
        let n = self.enemy?;
        let e = view.entities().find(|x| x.entity_number == n)?;
        Some(e.origin.z - view.self_state().origin.z)
    }

    /// Is there a transient item within Q3's nearby-goal range (~150 u)?
    fn item_nearby(&self, view: &crate::perception::Worldview) -> bool {
        !view.items_in_range(150.0).is_empty()
    }

    /// Switch FSM node, recording per-node entry timers (`chase_time`, `nbg_time`). Returns
    /// `true` so the caller's switch loop counts it.
    fn enter(&mut self, node: Q3Node) -> bool {
        match node {
            Q3Node::BattleChase => self.chase_deadline = self.time + 10.0,
            Q3Node::SeekNbg | Q3Node::BattleNbg => self.nbg_deadline = self.time + 5.0,
            Q3Node::SeekLtg => self.enemy = None,
            _ => {}
        }
        self.node = node;
        true
    }

    /// Goal for a battle node: the enemy's current origin if known, else the last place we saw
    /// it, else hold.
    fn battle_goal(&self, view: &crate::perception::Worldview) -> NavGoal {
        if let Some(n) = self.enemy {
            if let Some(e) = view.entities().find(|x| x.entity_number == n) {
                return NavGoal::Entity(e.origin);
            }
        }
        if let Some(p) = self.last_enemy_pos {
            return NavGoal::Position(p);
        }
        NavGoal::Position(view.self_state().origin)
    }

    /// Goal while retreating: grab the best nearby item if one exists (back away *toward* a
    /// pickup, Q3 `BotLongTermGoal(retreat=true)`); otherwise step directly away from the enemy.
    fn retreat_goal(&self, view: &crate::perception::Worldview) -> NavGoal {
        if let Some((item_pos, _)) = items::best_item_goal(view, &self.item_skill) {
            return NavGoal::Position(item_pos);
        }
        let pos = view.self_state().origin;
        let away = self
            .last_enemy_pos
            .map(|enemy| (pos - enemy).normalize_or_zero())
            .unwrap_or(Vec3::X);
        NavGoal::Position(pos + away * 256.0)
    }

    /// **`BotFindEnemy`** (`ai_dmq3.c:2929`, distilled §4) — pick the best enemy from the
    /// PVS-limited `view.enemies()`:
    /// 1. **Range gate by ALERTNESS:** skip enemies past `√` of `(900 + alertness·4000)²`.
    /// 2. **Closest-preference:** never switch to an enemy farther than the current one.
    /// 3. **Awareness FOV:** full 360° the frame our health dropped (we notice who's hurting
    ///    us); otherwise a cone that widens up close and narrows (~90°) at range.
    /// 4. **Visibility:** LOS trace (zero-size) when a collision model is present.
    /// 5. **Sneak-past:** a distant (`>100u`) enemy who isn't looking at us and whom we'd rather
    ///    not fight (`wants_to_retreat`) is skipped — we slip by instead of committing.
    ///
    /// On a fresh sight `enemy_first_seen` is set to *now* (drives the reaction-time gate); when
    /// *upgrading* from an existing enemy it's set to `now − 2 s` so the reaction delay doesn't
    /// re-trigger mid-fight (Q3 `enemysight_time = now − 2`). "Enemy is shooting" is not
    /// wire-observable, so it's treated as `false` (conservative); the health-drop branch still
    /// grants full awareness when we actually take damage.
    fn select_enemy(
        &mut self,
        view: &crate::perception::Worldview,
        cm: Option<&world::CollisionModel>,
        took_damage: bool,
    ) {
        let self_pos = view.self_state().origin;
        let eye = crate::los::eye_origin(self_pos.into());
        let our_yaw = view.self_state().angles.y;
        let our_forward = crate::steer::view_forward(our_yaw);

        let alert_range = 900.0 + self.ch.alertness * 4000.0;
        let alert_range_sq = alert_range * alert_range;

        // Distance² to the current enemy (closest-preference baseline).
        let cur_dist_sq = self
            .enemy
            .and_then(|n| {
                view.entities()
                    .find(|e| e.entity_number == n && !e.is_stale)
            })
            .map(|e| (e.origin - self_pos).length_squared());

        let mut best: Option<(i32, Vec3, f32)> = None;
        for e in view.enemies() {
            let to = e.origin - self_pos;
            let d2 = to.length_squared();
            if d2 > alert_range_sq {
                continue; // out of detection range
            }
            if matches!(cur_dist_sq, Some(cd) if d2 > cd) {
                continue; // don't switch to a farther enemy than the current
            }
            let dist = d2.sqrt();

            // Awareness FOV: 360° on damage, else 150° close → 90° far.
            let awareness_fov = if took_damage {
                360.0
            } else {
                (150.0 - (dist / alert_range) * 60.0).clamp(90.0, 150.0)
            };
            if !fov_cone(our_forward, to, awareness_fov) {
                continue;
            }

            // Line of sight (zero-size trace) when geometry is loaded.
            if let Some(cm) = cm {
                if !crate::los::has_los_player(cm, eye, e.origin.into()) {
                    continue;
                }
            }

            // Sneak-past: distant enemy not looking at us + we'd rather retreat → skip.
            if dist > 100.0 && !took_damage {
                let enemy_forward = crate::steer::view_forward(e.angles.y);
                let in_their_fov = fov_cone(enemy_forward, self_pos - e.origin, 90.0);
                if !in_their_fov {
                    let hdelta = Some(e.origin.z - self_pos.z);
                    if crate::q3char::wants_to_retreat(view, &self.ch, hdelta) {
                        continue;
                    }
                }
            }

            if best.map(|(_, _, bd)| d2 < bd).unwrap_or(true) {
                best = Some((e.entity_number, e.origin, d2));
            }
        }

        if let Some((n, origin, _)) = best {
            if self.enemy != Some(n) {
                // Fresh sight → reaction timer starts now; upgrade → now−2 (don't re-trigger).
                self.enemy_first_seen = Some(if self.enemy.is_some() {
                    self.time - 2.0
                } else {
                    self.time
                });
            }
            self.enemy = Some(n);
            self.last_enemy_pos = Some(origin);
            self.enemy_seen_time = self.time;
        }
    }

    /// Apply one round of node transitions (distilled §1), looping until the node is stable or
    /// the `MAX_NODESWITCHES` guard trips. `enemy_visible` is this frame's LOS+sight result.
    fn run_fsm(
        &mut self,
        view: &crate::perception::Worldview,
        wants_retreat: bool,
        wants_chase: bool,
        enemy_visible: bool,
    ) {
        let enemy_alive = self.enemy_alive(view);
        let item_nearby = self.item_nearby(view);
        let mut switches = 0u32;
        loop {
            let switched = match self.node {
                Q3Node::SeekLtg => {
                    if enemy_visible {
                        let next = if wants_retreat {
                            Q3Node::BattleRetreat
                        } else {
                            Q3Node::BattleFight
                        };
                        self.enter(next)
                    } else if item_nearby && self.time >= self.next_nbg_check {
                        self.next_nbg_check = self.time + 0.5;
                        self.enter(Q3Node::SeekNbg)
                    } else {
                        false
                    }
                }
                Q3Node::SeekNbg => {
                    if enemy_visible {
                        let next = if wants_retreat {
                            Q3Node::BattleRetreat
                        } else {
                            Q3Node::BattleFight
                        };
                        self.enter(next)
                    } else if self.time >= self.nbg_deadline || !item_nearby {
                        self.enter(Q3Node::SeekLtg)
                    } else {
                        false
                    }
                }
                Q3Node::BattleFight => {
                    if !enemy_alive {
                        self.enter(Q3Node::SeekLtg)
                    } else if !enemy_visible {
                        let next = if wants_chase {
                            Q3Node::BattleChase
                        } else {
                            Q3Node::SeekLtg
                        };
                        self.enter(next)
                    } else if wants_retreat {
                        self.enter(Q3Node::BattleRetreat)
                    } else if item_nearby && self.time >= self.next_nbg_check {
                        self.next_nbg_check = self.time + 0.5;
                        self.return_node = Q3Node::BattleFight;
                        self.enter(Q3Node::BattleNbg)
                    } else {
                        false
                    }
                }
                Q3Node::BattleChase => {
                    if enemy_visible {
                        self.enter(Q3Node::BattleFight)
                    } else if !enemy_alive || self.time >= self.chase_deadline {
                        self.enter(Q3Node::SeekLtg)
                    } else {
                        false
                    }
                }
                Q3Node::BattleRetreat => {
                    if wants_chase {
                        self.enter(Q3Node::BattleChase)
                    } else if !enemy_alive || (self.time - self.enemy_seen_time) > 4.0 {
                        self.enter(Q3Node::SeekLtg)
                    } else {
                        false
                    }
                }
                Q3Node::BattleNbg => {
                    if !enemy_alive {
                        self.enter(Q3Node::SeekLtg)
                    } else if self.time >= self.nbg_deadline || !item_nearby {
                        self.enter(self.return_node)
                    } else {
                        false
                    }
                }
            };
            if !switched {
                break;
            }
            switches += 1;
            if switches >= MAX_NODESWITCHES {
                tracing::warn!(node = self.node.label(), "Q3 FSM node-switch guard tripped");
                break;
            }
        }
    }

    /// Combat tick for a battle node with a **visible** enemy. Runs `BotAttackMove`,
    /// `BotAimAtEnemy`, and `BotCheckAttack` (distilled §5/§6): picks a weapon, aims with the Q3
    /// error model, decides fire (reaction gate, fire-throttle, self-preservation), and
    /// circle-strafes. `retreat` biases the movement backward (back away while still shooting if
    /// `attack_skill` allows). Returns a `use <weapon>` request if the desired weapon changed.
    fn combat_drive(
        &mut self,
        view: &crate::perception::Worldview,
        cm: Option<&world::CollisionModel>,
        pos: Vec3,
        dt: f32,
        retreat: bool,
        mv: &mut MovementIntent,
    ) -> Option<weapons::Weapon> {
        let n = self.enemy?;
        let e = view.entities().find(|x| x.entity_number == n)?;
        let enemy_pos = e.origin;
        let enemy_vel = e.velocity;
        let to = enemy_pos - pos;
        let dist = to.length().max(1.0);
        let dir = Vec3::new(to.x, to.y, 0.0).normalize_or_zero();

        // Weapon selection (optimistic; the server grants only owned weapons).
        // q3 keeps its Plan-38 baseline weapon model — opt out of Plan 30 T4 ammo gating.
        let desired = weapons::select_best_weapon(self.held_weapon, dist, i32::MAX);
        let weapon_request = if desired != self.held_weapon {
            self.held_weapon = desired;
            self.weapon_switch_time = self.time;
            Some(desired)
        } else {
            None
        };
        let weapon = self.held_weapon;

        // Aim (Q3 per-weapon accuracy + reaction gate + error model).
        let eye = Vec3::from(crate::los::eye_origin(pos.into()));
        let sighted = self
            .enemy_first_seen
            .map(|t| self.time - t)
            .unwrap_or(0.0)
            .max(0.0);
        let mut rng =
            crate::aim::JitterRng::new((self.time * 1000.0) as u32 ^ self.rng_state ^ (n as u32));
        let aimres = aim::aim_at_enemy(
            &mut self.aim,
            &aim::AimInput {
                ch: &self.ch,
                weapon,
                shooter_eye: eye,
                enemy_origin: enemy_pos,
                enemy_vel,
                sighted_secs: sighted,
                visible: true,
                time: self.time,
                cm,
            },
            &mut rng,
        );
        // Snap the view to the aim (the engine snaps); keep steering's integrator in sync.
        self.steering.set_view_yaw(aimres.yaw);
        mv.look_at(aimres.yaw, aimres.pitch);

        // Fire decision.
        if self.check_attack(weapon, dist, eye, pos, enemy_pos, aimres, cm) {
            mv.attack();
        }

        // Movement: circle-strafe (Q3 `BotAttackMove`), biased backward while retreating.
        let flip = self.roll();
        let backup = self.roll();
        let mut world =
            r#move::attack_move(&self.ch, dist, dir, &mut self.strafe, dt, flip, backup);
        if retreat {
            world = (world - dir).normalize_or_zero();
        }
        // Never strafe/backpedal into lava or off a blind drop (Plan 48 L2): mirror the
        // tangential component across the enemy axis, else stand and fight.
        if let Some(c) = cm {
            if crate::hazard::dir_is_hazardous(c, pos, world) {
                let radial = dir * world.dot(dir);
                let mirrored = (2.0 * radial - world).normalize_or_zero();
                world = if !crate::hazard::dir_is_hazardous(c, pos, mirrored) {
                    // Keep the flip for future ticks so we don't re-pick the deadly side.
                    self.strafe.dir = -self.strafe.dir;
                    mirrored
                } else {
                    // Both strafe sides deadly (pool rim): retreat from the rim while the
                    // aim stays locked — standing still gets juggled in (Plan 50).
                    crate::hazard::rim_retreat_dir(c, pos, aimres.yaw).unwrap_or(Vec3::ZERO)
                };
            }
        }
        let (fwd, side) = move_from_world_dir(world, aimres.yaw, false);
        mv.move_forward(fwd);
        mv.move_side(side);

        // Jump / crouch dodge with 1 s cooldowns (CROUCHER is best-effort — see brain_notes).
        if self.roll() < self.ch.jumper && self.time >= self.next_jump_time {
            mv.jump();
            self.next_jump_time = self.time + 1.0;
        }
        if self.roll() < self.ch.croucher && self.time >= self.next_crouch_time {
            mv.crouch = true;
            self.next_crouch_time = self.time + 1.0;
        }

        weapon_request
    }

    /// `BotCheckAttack` (`ai_dmq3.c:3555`, distilled §6): fire iff the reaction gate, the
    /// weapon-change lockout, the "facing the aim target" FOV gate, the LOS trace, the range
    /// sanity, the **self-preservation** splash abort, and the **fire-throttle duty cycle** all
    /// permit it.
    #[allow(clippy::too_many_arguments)] // a cohesive fire-decision bundle; splitting hurts clarity
    fn check_attack(
        &mut self,
        weapon: weapons::Weapon,
        dist: f32,
        eye: Vec3,
        self_pos: Vec3,
        enemy_pos: Vec3,
        aim: aim::AimResult,
        cm: Option<&world::CollisionModel>,
    ) -> bool {
        // Reaction-time sight gate (all skills) + the high-skill aim-not-ready gate.
        let sighted = self.enemy_first_seen.map(|t| self.time - t).unwrap_or(0.0);
        if sighted < self.ch.reaction_time || !aim.ready {
            return false;
        }
        // Don't fire mid weapon-change (0.1 s).
        if self.time - self.weapon_switch_time < 0.1 {
            return false;
        }
        // Must be roughly looking at the aim target (120° if close, else 50°).
        let fov = if dist < 100.0 { 120.0 } else { 50.0 };
        if !fov_cone(
            crate::steer::view_forward(aim.yaw),
            enemy_pos - self_pos,
            fov,
        ) {
            return false;
        }
        // LOS unblocked.
        if let Some(cm) = cm {
            if !crate::los::has_los_player(cm, eye.into(), enemy_pos.into()) {
                return false;
            }
        }
        // Range sanity.
        if dist < weapon.min_safe_distance() {
            return false;
        }
        if weapon == weapons::Weapon::Blaster && dist > weapon.effective_range() {
            return false;
        }
        // Self-preservation: don't rocket our own feet near a wall.
        if let Some(cm) = cm {
            if self.ch.self_preservation > 0.3
                && aim::would_self_splash(cm, eye, self_pos, enemy_pos, weapon)
            {
                return false;
            }
        }
        // Fire-throttle duty cycle.
        self.throttle_allows()
    }

    /// The fire-throttle duty cycle (`FIRETHROTTLE`, distilled §6.3): maintain alternating
    /// shoot/wait windows whose lengths come from the characteristic — humanizes sustained fire.
    /// `random() > throttle` → a `throttle`-second *wait*; else a `(1−throttle)`-second *shoot*.
    fn throttle_allows(&mut self) -> bool {
        if self.time >= self.throttle_until {
            let ft = self.ch.firethrottle.clamp(0.0, 1.0);
            if self.roll() > ft {
                self.throttle_firing = false;
                self.throttle_until = self.time + ft.max(0.05);
            } else {
                self.throttle_firing = true;
                self.throttle_until = self.time + (1.0 - ft).max(0.05);
            }
        }
        self.throttle_firing
    }
}

impl Brain for Q3Brain {
    fn set_map(&mut self, map: BrainMap) {
        let BrainMap {
            roam_nodes,
            nav_graph,
            roam_as_position,
            items: _, // q3 uses its own neutral PVS item picker (Plan 30 keeps q3 baseline)
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

        let pos = view.self_state().origin;
        let health = view.self_state().health;
        let took_damage = health < self.last_health;

        // ── 1. Perceive: acquire/refresh the enemy + this frame's aggression decision ──
        self.select_enemy(view, cm, took_damage);
        let enemy_visible = self.enemy_visible(view, cm);
        let hdelta = self.enemy_height_delta(view);
        let wants_retreat = crate::q3char::wants_to_retreat(view, &self.ch, hdelta);
        let wants_chase = crate::q3char::wants_to_chase(view, &self.ch, hdelta);

        // ── 2. Node FSM transitions (aggression-gated) ──
        self.run_fsm(view, wants_retreat, wants_chase, enemy_visible);

        // ── 3. Drive movement per node: combat (visible enemy) vs path-following ──
        let mut mv = MovementIntent::new();
        let mut weapon_request = None;
        if let Some(nav) = nav {
            if let Some(g) = goal_override.clone() {
                // Scenario / pinned-goal override always path-follows.
                self.locomote(nav, cm, pos, g, dt, view, &mut mv);
            } else {
                match self.node {
                    Q3Node::BattleFight if enemy_visible => {
                        // Keep nav warm (a path to the enemy) for an instant chase if LOS drops.
                        nav.update(pos, None);
                        nav.set_goal(self.battle_goal(view), pos);
                        weapon_request = self.combat_drive(view, cm, pos, dt, false, &mut mv);
                    }
                    Q3Node::BattleRetreat if enemy_visible => {
                        nav.update(pos, None);
                        nav.set_goal(self.retreat_goal(view), pos);
                        weapon_request = self.combat_drive(view, cm, pos, dt, true, &mut mv);
                    }
                    other => {
                        let goal = match other {
                            Q3Node::SeekNbg | Q3Node::BattleNbg => {
                                items::best_item_goal(view, &self.item_skill)
                                    .map(|(p, _)| NavGoal::Position(p))
                                    .unwrap_or_else(|| self.roam_goal(view, ticks, pos))
                            }
                            // Chase / lost-sight fight / retreat → path toward the goal.
                            Q3Node::BattleFight | Q3Node::BattleChase => self.battle_goal(view),
                            Q3Node::BattleRetreat => self.retreat_goal(view),
                            _ => self.roam_goal(view, ticks, pos),
                        };
                        self.locomote(nav, cm, pos, goal, dt, view, &mut mv);
                    }
                }
            }
        } else {
            // No nav graph yet — walk forward so the bot isn't a statue.
            mv.move_forward(1.0);
        }

        // Survival override (Plan 50 E2): standing in lava/slime outranks combat, dodge,
        // and route — face the nearest safe floor, sprint, swim up, and jump the pool rim.
        // Health-gated: a sinking corpse still ticks — don't "escape" (or log) post-mortem.
        if let Some(c) = cm {
            if let Some(esc) = (health > 0)
                .then(|| crate::hazard::escape_from_lava(c, pos))
                .flatten()
            {
                let yaw = esc.y.atan2(esc.x).to_degrees();
                self.steering.set_view_yaw(yaw);
                mv.look_at(yaw, 0.0);
                mv.forward = 1.0;
                mv.side = 0.0;
                mv.jump();
                mv.up = 1.0;
                tracing::info!(
                    x = pos.x as i32,
                    y = pos.y as i32,
                    z = pos.z as i32,
                    "EVT lava_escape"
                );
            }
        }

        self.last_health = health;

        BrainOutput {
            intent: mv,
            weapon_request,
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

    /// A view with one visible enemy in the open + a held weapon/health that the caller chooses,
    /// so the FSM can be driven deterministically with no server/cm (LOS optimistic).
    fn view_enemy(view_model: &str, health: i16, ammo: i16) -> Worldview {
        use q2proto::EntityState;
        let mut frame = Frame::default();
        frame.playerstate.gunindex = 1;
        frame.playerstate.stats[1] = health;
        frame.playerstate.stats[3] = ammo;
        frame.playerstate.stats[5] = 100; // armor
        frame.entities = vec![EntityState {
            number: 9,
            origin: [200.0, 0.0, 0.0],
            angles: [0.0, 180.0, 0.0], // facing −x, toward the bot at the origin
            modelindex: 255,           // player
            ..Default::default()
        }];
        let mut cs = ConfigStrings::default();
        cs.set(32 + 1, view_model);
        Worldview::from_frame(&frame, &cs, 0)
    }

    fn drive(b: &mut Q3Brain, view: &Worldview) {
        let mut nav = StubNav {
            pursue: Some(Vec3::new(200.0, 0.0, 0.0)),
            ..Default::default()
        };
        b.tick(BrainContext {
            view,
            nav: Some(&mut nav),
            cm: None, // optimistic LOS so the enemy counts as visible
            dt: 0.1,
            ticks: 1,
            goal_override: None,
        });
    }

    #[test]
    fn high_aggression_enemy_enters_fight() {
        // Sarge (high aggression) + railgun + slugs + full health → wants_chase → Fight.
        let mut b = Q3Brain::new(Q3Character::sarge());
        let view = view_enemy("models/weapons/v_rail/tris.md2", 100, 8);
        drive(&mut b, &view);
        assert_eq!(b.status(), "fight");
        assert_eq!(b.enemy, Some(9));
    }

    #[test]
    fn out_gunned_enemy_enters_retreat() {
        // Machinegun (tier<50 → aggression 0) → wants_retreat → Retreat on contact.
        let mut b = Q3Brain::new(Q3Character::default());
        let view = view_enemy("models/weapons/v_machn/tris.md2", 100, 200);
        drive(&mut b, &view);
        assert_eq!(b.status(), "retreat");
    }

    #[test]
    fn fight_loses_sight_chases_when_aggressive() {
        let mut b = Q3Brain::new(Q3Character::sarge());
        let view = view_enemy("models/weapons/v_rail/tris.md2", 100, 8);
        drive(&mut b, &view); // → Fight
        assert_eq!(b.status(), "fight");
        // Now the enemy goes stale (out of PVS) → not visible but still wants_chase → Chase.
        let mut stale = view.clone();
        for e in stale.entities_mut() {
            if e.entity_number == 9 {
                e.is_stale = true;
            }
        }
        drive(&mut b, &stale);
        assert_eq!(b.status(), "chase");
    }

    #[test]
    fn enemy_gone_returns_to_seek() {
        let mut b = Q3Brain::new(Q3Character::sarge());
        let view = view_enemy("models/weapons/v_rail/tris.md2", 100, 8);
        drive(&mut b, &view); // → Fight
                              // Enemy entity vanishes entirely (dead) → SeekLtg.
        let empty = empty_view();
        drive(&mut b, &empty);
        assert_eq!(b.status(), "seek-ltg");
        assert!(b.enemy.is_none());
    }

    /// Build a view with one enemy at a chosen distance + facing yaw + our health.
    fn view_enemy_at(
        dist: f32,
        enemy_yaw: f32,
        view_model: &str,
        ammo: i16,
        health: i16,
    ) -> Worldview {
        use q2proto::EntityState;
        let mut frame = Frame::default();
        frame.playerstate.gunindex = 1;
        frame.playerstate.stats[1] = health;
        frame.playerstate.stats[3] = ammo;
        frame.playerstate.stats[5] = 100;
        frame.entities = vec![EntityState {
            number: 9,
            origin: [dist, 0.0, 0.0],
            angles: [0.0, enemy_yaw, 0.0],
            modelindex: 255,
            ..Default::default()
        }];
        let mut cs = ConfigStrings::default();
        cs.set(32 + 1, view_model);
        Worldview::from_frame(&frame, &cs, 0)
    }

    #[test]
    fn sneak_past_distant_outgunned_enemy_not_facing_us() {
        // Default char, machinegun (wants_retreat), distant (200u), enemy facing AWAY (+x) →
        // not in their FOV + we'd rather retreat → sneak past → stay SeekLtg.
        let mut b = Q3Brain::new(Q3Character::default());
        let view = view_enemy_at(200.0, 0.0, "models/weapons/v_machn/tris.md2", 200, 100);
        drive(&mut b, &view);
        assert_eq!(b.status(), "seek-ltg");
        assert!(b.enemy.is_none(), "did not commit to the sneak-past enemy");
    }

    #[test]
    fn alertness_range_gates_far_enemy() {
        // Default alertness 0.6 → range 900 + 0.6·4000 = 3300u. An enemy at 4000u (facing us,
        // railgun) is out of detection range → not engaged.
        let mut b = Q3Brain::new(Q3Character::default());
        let view = view_enemy_at(4000.0, 180.0, "models/weapons/v_rail/tris.md2", 8, 100);
        drive(&mut b, &view);
        assert_eq!(b.status(), "seek-ltg");
        assert!(b.enemy.is_none());
    }

    #[test]
    fn taking_damage_grants_full_awareness() {
        // Enemy facing AWAY (would normally be sneak-past), but we took damage this frame → 360°
        // awareness commits to them regardless. Machinegun → out-gunned → Retreat.
        let mut b = Q3Brain::new(Q3Character::default());
        b.last_health = 100; // previous frame
        let view = view_enemy_at(200.0, 0.0, "models/weapons/v_machn/tris.md2", 200, 70); // hurt
        drive(&mut b, &view);
        assert_eq!(
            b.enemy,
            Some(9),
            "committed despite the enemy not facing us"
        );
        assert_eq!(b.status(), "retreat");
    }
}
