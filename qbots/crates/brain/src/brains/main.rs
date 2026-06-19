//! # brain::brains::main — the `main` brain plugin (the "normal" bot; Plan 22/24)
//!
//! `MainBrain` owns every per-tick *decision* sub-driver that used to live as locals in the
//! fleet binary's `bot_task` loop (`crates/qbots/src/main.rs`): the combat driver, the
//! behavior FSM, the projectile-dodge driver, the steering controller, stuck recovery,
//! the per-bot skill/personality, and the roam goal cursor. The `Navigator` (nav) is
//! **injected** into [`MainBrain::tick`] each frame — the brain *uses* nav to reach a goal but
//! never owns or mutates the nav graph. The driver (`MovementIntent → Usercmd`) stays on
//! the far side of the seam: `tick` returns a [`BrainOutput`] and the caller assembles the
//! `Usercmd`.
//!
//! This is a behavior-preserving extraction: with [`BrainConfig::default`] the logic here
//! reproduces the pre-extraction `bot_task` body byte-for-byte. `BrainConfig::combat_enabled`
//! lets the movement-scenario runner disable combat; the pinned goal arrives per-tick via
//! [`BrainContext::goal_override`] (Plan 26 — the scenario resolves it lazily).

use std::sync::Arc;

use glam::Vec3;
use world::NavGraph;

use crate::brains::core::{BrainContext, BrainMap};
use crate::combat::{CombatDecision, CombatDriver};
use crate::danger::DangerDriver;
use crate::fsm::{BehaviorIntent, BehaviorState};
use crate::move_ctrl::MovementIntent;
use crate::nav::NavGoal;
use crate::recover::{Recovery, RecoveryAction};
use crate::skill::BotSkill;
use crate::steer::{move_from_world_dir, Steering};
use crate::water::{is_swimming, water_level, EXIT_LOOKUP_PITCH, SWIM_VERT_SCALE};
use crate::{items, los};

// `BrainConfig`/`BrainOutput` live in `brains::core` next to the `trait Brain` contract;
// re-exported here for the convenience of code that reaches them via the `main` module.
pub use crate::brains::core::{BrainConfig, BrainOutput};

/// The `main` decision brain: owns combat/FSM/dodge/steering/recovery/skill/roam state.
pub struct MainBrain {
    skill: BotSkill,
    fsm: BehaviorState,
    combat: CombatDriver,
    danger: DangerDriver,
    steering: Steering,
    recovery: Recovery,
    /// Roam goal cursor (node indices into the A* graph) + position in it.
    roam_nodes: Vec<usize>,
    roam_idx: usize,
    /// The A* graph handle, kept so the navmesh backend can resolve a roam node index to
    /// a world position. `None` until the map loads.
    nav_graph: Option<Arc<NavGraph>>,
    /// `true` when the active nav backend (navmesh) cannot path to a bare node index, so
    /// roam goals are expressed as world positions instead. Set at map load.
    roam_as_position: bool,
    cfg: BrainConfig,
}

impl MainBrain {
    /// Construct a brain before the map is known. Roam goals + the graph handle are
    /// supplied later via [`set_map`](crate::brains::core::Brain::set_map) (mirrors how
    /// `bot_task` built its sub-drivers early and learned the nav graph at map load).
    pub fn new(skill: BotSkill, cfg: BrainConfig) -> Self {
        let steering = Steering::new(skill.combat());
        Self {
            skill,
            fsm: BehaviorState::Roam,
            combat: CombatDriver::new(),
            danger: DangerDriver::new(),
            steering,
            recovery: Recovery::new(),
            roam_nodes: Vec::new(),
            roam_idx: 0,
            nav_graph: None,
            roam_as_position: false,
            cfg,
        }
    }

    /// The current behavior state (typed). The public, FSM-agnostic label is the trait's
    /// [`status`](crate::brains::core::Brain::status); this stays only for the unit test that
    /// asserts the typed state.
    #[cfg(test)]
    pub(crate) fn behavior(&self) -> &BehaviorState {
        &self.fsm
    }
}

impl crate::brains::core::Brain for MainBrain {
    /// Supply the per-map roam goals + A* graph handle once the map has loaded.
    /// `roam_as_position` is `true` for backends (navmesh) that path to world positions
    /// rather than bare node indices.
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

    /// The danger/popularity heatmap cost weights for this bot's personality — the caller
    /// feeds these into the nav risk overlay.
    fn heatmap_weights(&self) -> (f32, f32) {
        self.skill.heatmap_weights()
    }

    /// Short FSM-derived status label (replaces the old typed `behavior()` in the periodic
    /// log; the core trait stays decoupled from `BehaviorState`).
    fn status(&self) -> &str {
        match self.fsm {
            BehaviorState::Roam => "roam",
            BehaviorState::Hunt { .. } => "hunt",
            BehaviorState::Engage { .. } => "engage",
            BehaviorState::Flee => "flee",
            BehaviorState::Pickup { .. } => "pickup",
        }
    }

    /// React to scoring a frag (Eraser auto-skill bump).
    fn on_kill(&mut self) {
        self.skill.on_kill();
    }

    /// React to dying: reset the held-weapon tracking to the respawn loadout and ease the
    /// auto-skill down (Eraser).
    fn on_death(&mut self) {
        self.combat.on_respawn();
        self.skill.on_death();
    }

    /// Decide one frame. `ctx.nav` is the injected navigator (None before the map loads).
    ///
    /// This is the lifted `bot_task` decision/steering body (Plan 22): combat eval →
    /// combat→FSM override → goal selection → ideal-yaw → circle-strafe/back-up → arrive
    /// throttle → forward/side decomposition → stuck recovery → jump-edge → projectile
    /// dodge. Behavior-preserving under [`BrainConfig::default`].
    fn tick(&mut self, ctx: BrainContext) -> BrainOutput {
        let BrainContext {
            view,
            nav,
            cm,
            dt,
            ticks,
            goal_override,
        } = ctx;
        let jitter = (ticks as f32) * 0.1;
        let combat_dec = if self.cfg.combat_enabled {
            self.combat.evaluate(view, &self.skill, jitter, cm)
        } else {
            CombatDecision::default()
        };

        // Pass combat target to FSM for navigation goal.
        // Only chase via nav when LOS holds (Plan 11 T4) — without
        // LOS the bot was walking into walls toward walled enemies.
        let fsm_intent = if let Some(target) = combat_dec.target_entity {
            let target_entity = view.entities().find(|e| e.entity_number == target);
            let target_pos = target_entity
                .map(|e| e.origin)
                .unwrap_or(view.self_state().origin);

            // LOS check: only set Entity nav goal when the path is clear.
            let has_los = target_entity
                .and_then(|te| {
                    cm.map(|cm| {
                        let eye = los::eye_origin(view.self_state().origin.into());
                        los::has_los_player(cm, eye, te.origin.into())
                    })
                })
                .unwrap_or(true); // no cm yet → optimistic (old behavior)

            if has_los {
                if !matches!(self.fsm, BehaviorState::Engage { .. }) {
                    tracing::debug!("forcing FSM into Engage (target={})", target);
                    self.fsm = BehaviorState::Engage {
                        target_entity: target,
                    };
                }
                tracing::trace!(
                    "combat target override: target={} pos={:?}",
                    target,
                    target_pos
                );
                BehaviorIntent {
                    nav_goal: Some(NavGoal::Entity(target_pos)),
                    should_pickup: None,
                }
            } else {
                // Target exists (grace-period fire still possible) but
                // no clear path → let FSM navigate (Hunt last-known pos).
                self.fsm.tick(view, cm)
            }
        } else {
            self.fsm.tick(view, cm)
        };

        let mut mv = MovementIntent::new();

        if combat_dec.should_fire {
            mv.attack();
        }

        let pos = view.self_state().origin;

        if let Some(nav) = nav {
            nav.update(pos, None);

            // Give-up watchdog: if we've chased this goal too long
            // without reaching a waypoint, abandon the current
            // combat target so we stop re-issuing the same stale
            // position and fall back to roaming.
            if nav.goal_abandoned() {
                self.combat.clear_target();
                self.fsm = BehaviorState::Roam;
            }

            let goal = if let Some(g) = goal_override.clone() {
                g
            } else if let Some(g) = fsm_intent.nav_goal {
                g
            } else if let Some((item_pos, _)) = items::best_item_goal(view, &self.skill) {
                // Seek the highest-value visible item (powerups,
                // armor, weapons) weighted by value/distance and
                // the bot's health need / quad_freak personality.
                NavGoal::Position(item_pos)
            } else if !self.roam_nodes.is_empty() {
                // Campers dwell ~5x longer per node (first-cut
                // camping; a true camp-node picker with cover/LOS
                // is a follow-up). Default roamer cycles every 5s.
                let dwell = if self.skill.camper { 250 } else { 50 };
                if ticks.is_multiple_of(dwell) {
                    self.roam_idx =
                        (self.roam_idx + self.roam_nodes.len() / 7 + 1) % self.roam_nodes.len();
                }
                let node = self.roam_nodes[self.roam_idx];
                // The navmesh backend doesn't index the A* graph's nodes, so
                // express the roam target as a world position it can path to.
                if self.roam_as_position {
                    match &self.nav_graph {
                        Some(g) => NavGoal::Position(Vec3::from(g.node_pos(node))),
                        None => NavGoal::Waypoint(node),
                    }
                } else {
                    NavGoal::Waypoint(node)
                }
            } else {
                NavGoal::Position(pos)
            };

            nav.set_goal(goal, pos);
            // String-pull the path into longer straight runs (Plan 14 T1).
            if let Some(cm) = cm {
                nav.smooth_with_cm(cm, pos);
            }

            // Ideal-distance combat constants (Eraser BOT_IDEAL_DIST_FROM_ENEMY).
            const IDEAL_DIST: f32 = 160.0;
            const BACKUP_DIST: f32 = 80.0;

            // Resolve enemy position + distance (if we have a target in view).
            let enemy_dist_dir: Option<(f32, Vec3)> = combat_dec.target_entity.and_then(|t| {
                view.entities().find(|e| e.entity_number == t).map(|enemy| {
                    let to = enemy.origin - pos;
                    let d = to.length();
                    let dir = if d > 1.0 { to / d } else { Vec3::X };
                    (d, dir)
                })
            });

            // ── 1. Ideal view yaw (priority: fire-aim > enemy-face > path) ──
            let (ideal_yaw, ideal_pitch) = if combat_dec.should_fire {
                (combat_dec.aim_yaw, combat_dec.aim_pitch)
            } else if let Some((d, dir)) = enemy_dist_dir {
                if d < IDEAL_DIST {
                    // Face enemy while in ideal-distance range.
                    let yaw = dir.y.atan2(dir.x).to_degrees();
                    (yaw, 0.0)
                } else {
                    // Far from enemy — steer along the path toward them.
                    nav.pursue_target(pos)
                        .filter(|pt| (pt - pos).length_squared() > 1.0)
                        .map(|pt| {
                            let d = pt - pos;
                            (d.y.atan2(d.x).to_degrees(), 0.0)
                        })
                        .unwrap_or((self.steering.view_yaw(), 0.0))
                }
            } else {
                // No combat: steer along the path.
                nav.pursue_target(pos)
                    .filter(|pt| (pt - pos).length_squared() > 1.0)
                    .map(|pt| {
                        let d = pt - pos;
                        (d.y.atan2(d.x).to_degrees(), 0.0)
                    })
                    .unwrap_or((self.steering.view_yaw(), 0.0))
            };

            // ── 2. Rate-limit the yaw turn toward ideal ───────────────────
            let view_yaw = self.steering.change_yaw(ideal_yaw, dt);
            mv.look_at(view_yaw, ideal_pitch);

            // ── 3. World move direction + face-then-go mode ───────────────
            // T5 circle-strafe: when Engage + LOS holds, separate aim (view_yaw →
            // enemy) from walk (radial ± tangential). Eraser: combat 1 = no strafe.
            let is_engage_los =
                combat_dec.should_fire || matches!(self.fsm, BehaviorState::Engage { .. });
            let strafe_weight = if is_engage_los && self.skill.combat() > 1.5 {
                0.7
            } else {
                0.0
            };

            let (world_move_dir, face_then_go) = if let Some((d, dir)) = enemy_dist_dir {
                if d < BACKUP_DIST {
                    // Back away from enemy while keeping aim on them.
                    let away = Vec3::new(-dir.x, -dir.y, 0.0).normalize_or_zero();
                    // Add tangential even while backing (keeps bot moving).
                    let tan = Vec3::new(-dir.y, dir.x, 0.0)
                        * self.steering.strafe_tick(dt)
                        * strafe_weight;
                    ((away + tan).normalize_or_zero(), false)
                } else if d < IDEAL_DIST {
                    // Hold ideal distance — pure circle-strafe tangentially.
                    let tan = Vec3::new(-dir.y, dir.x, 0.0) * self.steering.strafe_tick(dt);
                    (tan.normalize_or_zero() * strafe_weight, false)
                } else {
                    // Chase via nav look-ahead + light tangential strafe.
                    let nav_dir = nav
                        .pursue_target(pos)
                        .map(|pt| {
                            let d = pt - pos;
                            Vec3::new(d.x, d.y, 0.0).normalize_or_zero()
                        })
                        .unwrap_or(Vec3::ZERO);
                    if strafe_weight > 0.0 {
                        let tan = Vec3::new(-dir.y, dir.x, 0.0)
                            * self.steering.strafe_tick(dt)
                            * strafe_weight;
                        ((nav_dir + tan).normalize_or_zero(), false)
                    } else {
                        (nav_dir, true)
                    }
                }
            } else {
                // Roaming: follow path look-ahead.
                let dir = nav
                    .pursue_target(pos)
                    .map(|pt| {
                        let d = pt - pos;
                        Vec3::new(d.x, d.y, 0.0).normalize_or_zero()
                    })
                    .unwrap_or(Vec3::ZERO);
                (dir, true)
            };

            // ── 4. Arrive throttle (slows near final goal) ────────────────
            let arrive = nav
                .pursue_target(pos)
                .map(|pt| Steering::arrive_scale((pt - pos).length()))
                .unwrap_or(1.0);

            // ── 5. Decompose into view-relative (forward, side) ───────────
            let (fwd, side) = move_from_world_dir(world_move_dir, view_yaw, face_then_go);
            mv.move_forward(fwd * arrive);
            mv.move_side(side * arrive);

            // Swimming gate (Plan 40 T4): suspend stuck recovery in water — the StuckDetector
            // false-fires on slow swim/bob and find_best_direction steers AWAY from water.
            let swim_active =
                cm.is_some_and(|c| is_swimming(water_level(c, pos))) || nav.current_edge_is_swim();

            // ── 6. Stuck recovery (Plan 13) ───────────────────────────────
            let has_nav_target = nav.pursue_target(pos).is_some();
            let engaging = matches!(self.fsm, BehaviorState::Engage { .. });
            let rec_action = if swim_active {
                RecoveryAction::None
            } else {
                self.recovery
                    .evaluate(pos, dt, cm, view_yaw, has_nav_target, engaging)
            };
            match rec_action {
                RecoveryAction::None => {}
                RecoveryAction::Jump => {
                    tracing::debug!(?pos, "stuck — jump");
                    mv.jump();
                }
                RecoveryAction::Strafe { dir } => {
                    tracing::debug!(?pos, dir, "stuck — strafe");
                    mv.move_side(dir);
                }
                RecoveryAction::BackOffThenRepath => {
                    tracing::debug!(?pos, "stuck — back off + repath");
                    mv.move_forward(-0.5);
                    nav.force_replan();
                }
                RecoveryAction::UseHeading(yaw) => {
                    tracing::debug!(?pos, yaw, "no nav — steer free heading");
                    let r = yaw.to_radians();
                    let free_dir = Vec3::new(r.cos(), r.sin(), 0.0);
                    let (hfwd, hside) = move_from_world_dir(free_dir, view_yaw, true);
                    mv.move_forward(hfwd);
                    mv.move_side(hside);
                }
            }

            // ── 7. Jump-edge activation (Plan 14 T2) ─────────────────────
            if nav.current_edge_is_jump() {
                mv.jump();
            }

            // ── 8. Swim activation (Plan 40) ─────────────────────────────
            // Live bots also swim water routes. Drive sustained vertical thrust toward the
            // 3-D look-ahead; never the one-shot jump in water. Combat aim wins the view pitch
            // when firing (Risk #3) — otherwise pitch toward the 3-D target so `pml.forward`
            // carries the vertical component for surfacing / climb-out.
            let water = cm.map_or(0, |c| water_level(c, pos));
            if let Some(cm) = cm {
                if is_swimming(water) || nav.current_edge_is_swim() {
                    let target = nav.pursue_target(pos).unwrap_or(pos);
                    let to = target - pos;
                    let dz = to.z;
                    let exiting =
                        nav.current_edge_is_swim() && dz > 0.0 && water_level(cm, target) == 0;
                    if exiting {
                        mv.up = 1.0;
                        if !combat_dec.should_fire {
                            mv.pitch = EXIT_LOOKUP_PITCH;
                        }
                    } else {
                        mv.up = (dz / SWIM_VERT_SCALE).clamp(-1.0, 1.0);
                        if !combat_dec.should_fire {
                            let hd = to.truncate().length().max(1.0);
                            mv.pitch = (-dz.atan2(hd).to_degrees()).clamp(-89.0, 89.0);
                        }
                    }
                    mv.jump = false; // jumping in water is a useless one-shot launch
                }
            }
        } else if !combat_dec.should_fire {
            // No nav graph loaded yet — just walk forward.
            mv.move_forward(1.0);
            if ticks.is_multiple_of(20) {
                mv.jump();
            }
        }

        // Tactical override: dodge an incoming projectile. This is
        // frame-scale and takes precedence over nav/engage intent.
        // The dodge direction (world space) is projected onto the
        // bot's right vector → a view-relative `side` strafe so we
        // keep facing the target while stepping off the line.
        let dodge = self.danger.evaluate(view, self.skill.combat());
        if dodge.is_active() {
            tracing::debug!(?dodge.strafe_dir, jump = dodge.jump, "dodging projectile");
            let yaw_rad = mv.yaw.to_radians();
            let right = Vec3::new(yaw_rad.sin(), -yaw_rad.cos(), 0.0);
            mv.side = dodge.strafe_dir.dot(right).clamp(-1.0, 1.0);
            mv.forward = 0.0;
            if dodge.jump {
                mv.jump();
            }
        }

        BrainOutput {
            intent: mv,
            weapon_request: combat_dec.weapon_request.map(|r| r.0),
            // The live fleet ignores this; for the scenario `--brain main` A/B it reports the
            // final forward intent.
            intent_forward: mv.forward,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_combat_on() {
        let cfg = BrainConfig::default();
        assert!(cfg.combat_enabled);
    }

    #[test]
    fn goal_override_drives_the_navigator() {
        use crate::brains::core::Brain as _;
        use crate::nav_mode::StubNav;
        use crate::perception::Worldview;
        use client::parse::ConfigStrings;
        use q2proto::Frame;

        // Combat off so the tick is pure navigation; the per-tick override must win the goal
        // ladder and be handed to the navigator verbatim.
        let mut brain = MainBrain::new(
            BotSkill::default(),
            BrainConfig {
                combat_enabled: false,
            },
        );
        let view = Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0);
        let mut nav = StubNav::default();
        let goal = NavGoal::Position(Vec3::new(123.0, 456.0, 0.0));
        let _ = brain.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: Some(goal.clone()),
        });
        assert_eq!(nav.last_goal, Some(goal));
    }

    #[test]
    fn new_brain_starts_roaming_without_map() {
        let brain = MainBrain::new(BotSkill::default(), BrainConfig::default());
        assert!(matches!(brain.behavior(), BehaviorState::Roam));
        assert!(brain.roam_nodes.is_empty());
        assert!(!brain.roam_as_position);
    }
}
