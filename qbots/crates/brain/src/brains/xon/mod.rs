//! # brain::brains::xon — the Xonotic-derived brain (`xon`; Plan 60)
//!
//! Ports havocbot's *decision texture* (research: `context/distilled/xonotic.md`; vendor:
//! `vendor/xonotic/data/xonotic-data.pk3dir/qcsrc/server/bot/default/`) onto qbots'
//! shared infrastructure. What makes `xon` different from `main`/`q3`/`zb2` (distilled §9):
//!
//! - **One smooth objective** — every candidate goal (item, enemy, wander waypoint) is
//!   rated `value * rangebias/(rangebias + travel_time)` and the best wins; no FSM picking
//!   goal *categories* (T2, [`crate::xoncore::rating`]).
//! - **Re-planning on evidence** — observed pickups, a 0.5 s goal-progress watchdog, and a
//!   3 s ignore-list replace pure re-plan timers (T2).
//! - **Aim as a dynamical system** — the five-stage anticipation cascade + mouse-think +
//!   fire cone ([`crate::xoncore::aim::XonAim`], T4).
//! - **Keyboard-emulated movement** ([`crate::xoncore::keyboard::KeyboardEmu`], T5).
//! - **Weapon combos** — switch mid-refire when another gun lands sooner (T3).
//!
//! Locomotion is a copy of `q3`'s proven `locomote` stage (Plan 58's shared extraction was
//! abandoned — see `context/plans/abandoned/58_*`): steering + hazard creep + traversal
//! gates + stuck recovery + jump edges, all delegating to the SAME shared modules
//! (`Steering`/`Recovery`/`TraversalExecutor`/`hazard`) every other brain uses, so `xon`
//! swims/rides/climbs and respects lava like the rest of the fleet.
//!
//! Personality: [`XonSkill`] — Xonotic's 12 additive skill axes (Plan 59).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use glam::Vec3;
use world::NavGraph;

use crate::brains::core::{Brain, BrainConfig, BrainContext, BrainMap, BrainOutput, MapItem};
use crate::items::{self, ItemMemory};
use crate::move_ctrl::MovementIntent;
use crate::nav::NavGoal;
use crate::perception::EntityClass;
use crate::recover::{Recovery, RecoveryAction};
use crate::skill::BotSkill;
use crate::steer::{move_from_world_dir, Steering};
use crate::traverse::{TraversalExecutor, TraversalFrame};
use crate::xonchar::XonSkill;
use crate::xoncore::aim::{AimInputs, Angles, XonAim};
use crate::xoncore::keyboard::KeyboardEmu;
use crate::xoncore::Lcg;
use crate::{aim as shared_aim, los};

mod combat;
mod dodge;
mod goals;
use combat::{EnemyTracker, WeaponChooser};
use goals::{RatingCtx, XonGoals};

/// Process-wide xon ordinal — staggers each bot's first rating session so a fleet doesn't
/// flood the graph on the same frame (the poor-man's strategy token, `bot.qc:784-811`).
static BOT_ORDINAL: AtomicUsize = AtomicUsize::new(0);

/// The Xonotic-derived decision brain. Owns the goal-stack strategy (T2), combat (T3–T5),
/// and the locomotion state; the `Navigator` is injected each tick.
pub struct XonBrain {
    /// The 12-axis personality (Plan 59) — every skill-scaled formula reads from here.
    sk: XonSkill,
    /// Deterministic per-bot RNG (the vendor's `random()`).
    rng: Lcg,
    /// Combat gate (scenarios force `combat_enabled = false` — enemies are then never
    /// rated as goals, and T3-T5 combat stays off).
    cfg: BrainConfig,

    // ── navigation / roam (mirrors Q3Brain until the T2 strategy layer lands) ─────────
    roam_nodes: Vec<usize>,
    roam_idx: usize,
    nav_graph: Option<Arc<NavGraph>>,
    roam_as_position: bool,
    /// Static BSP item table (Plan 30) — the rating-session candidates.
    map_items: Vec<MapItem>,
    /// PVS-honest taken/respawn memory over `map_items` (shared `items::ItemMemory`).
    item_memory: ItemMemory,
    /// The goal-stack strategy layer (T2).
    goals: XonGoals,
    /// Sticky enemy selection (T3).
    enemy: EnemyTracker,
    /// Priority-list weapon choice + probe-and-learn inventory (T3).
    weapon: WeaponChooser,
    /// When we last pulled the trigger (drives the combo check; set by T4's fire).
    fired_at: Option<f32>,
    /// The aim dynamical system (T4) — owns combat view angles + the fire timer.
    aim: XonAim,
    /// Current view pitch (yaw lives in `steering`); XonAim integrates from here.
    view_pitch: f32,
    /// Keyboard-emulation quantizer (T5) — the last stage before the intent is emitted.
    keyboard: KeyboardEmu,
    /// Low-skill overshoot stop deadline (`havocbot.qc:1130-1134`).
    overshoot_until: f32,
    /// Wall-clock seconds since connect (accumulated from `dt`).
    time: f32,
    /// Status label reflecting the committed goal kind (`xon-item`/`xon-enemy`/`xon-wander`).
    status: &'static str,

    // ── shared locomotion primitives (same modules as main/q3/zb2) ────────────────────
    steering: Steering,
    recovery: Recovery,
    traverse: TraversalExecutor,
    /// Skill for the shared PVS item picker used by the interim roam ladder.
    item_skill: BotSkill,
}

impl XonBrain {
    /// Build an `xon` brain with the given personality. Roam goals + the nav graph arrive
    /// later via [`set_map`](Brain::set_map).
    pub fn new(sk: XonSkill, cfg: BrainConfig) -> Self {
        Self::with_ordinal(sk, cfg, BOT_ORDINAL.fetch_add(1, Ordering::Relaxed))
    }

    /// Deterministic constructor: everything seeded from `ordinal` (tests pin it; `new`
    /// takes the process-wide counter).
    fn with_ordinal(sk: XonSkill, cfg: BrainConfig, ordinal: usize) -> Self {
        // Path-following turn rate scales with movement skill (XonAim owns combat turning
        // from T4); qport-independent, deterministic.
        let steering = Steering::new(1.0 + (sk.movement() / 10.0).clamp(0.0, 1.0) * 4.0);
        Self {
            sk,
            rng: Lcg::new(0x584f_4e21 ^ ordinal as u32), // "XON!" + per-bot ordinal
            cfg,
            roam_nodes: Vec::new(),
            roam_idx: 0,
            nav_graph: None,
            roam_as_position: false,
            map_items: Vec::new(),
            item_memory: ItemMemory::new(),
            goals: XonGoals::new(ordinal as f32 * 0.35),
            enemy: EnemyTracker::new(),
            weapon: WeaponChooser::new(),
            fired_at: None,
            aim: XonAim::new(),
            view_pitch: 0.0,
            keyboard: KeyboardEmu::new(),
            overshoot_until: 0.0,
            time: 0.0,
            status: "xon",
            steering,
            recovery: Recovery::new(),
            traverse: TraversalExecutor::new(),
            item_skill: BotSkill::default(),
        }
    }

    /// Interim roam ladder (T1; replaced by the T2 rating session): best visible item, else
    /// the roam cursor, else hold — the same shape every brain boots with.
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

    /// Run the T2 strategy layer: build this frame's rating context and delegate to
    /// [`XonGoals::tick`]. Enemies are candidates only when combat is enabled (the
    /// scenario contract). Returns `(goal, replan_requested)`.
    fn strategy_goal(
        &mut self,
        view: &crate::perception::Worldview,
        pos: Vec3,
        dt: f32,
    ) -> Option<(NavGoal, bool)> {
        let graph = self.nav_graph.clone()?;
        let enemies: Vec<(i32, Vec3)> = if self.cfg.combat_enabled {
            view.entities()
                .filter(|e| e.class == EntityClass::EnemyPlayer && !e.is_stale)
                // Don't rate rocketing/falling players as goals (`roles.qc:191`).
                .filter(|e| e.velocity.is_none_or(|v| v.length() <= 640.0))
                .map(|e| (e.entity_number, e.origin))
                .collect()
        } else {
            Vec::new()
        };
        let ss = view.self_state();
        let ctx = RatingCtx {
            graph: &graph,
            items: &self.map_items,
            memory: &self.item_memory,
            enemies: &enemies,
            roam_nodes: &self.roam_nodes,
            pos,
            health: ss.health as f32,
            armor: ss.armor as f32,
            held: ss.held_weapon.unwrap_or(crate::weapons::Weapon::Blaster),
            now: self.time,
        };
        let d = self.goals.tick(&mut self.rng, &self.sk, &ctx, dt)?;
        self.status = match d.key {
            goals::GoalKey::Item(_) => "xon-item",
            goals::GoalKey::Enemy(_) => "xon-enemy",
            goals::GoalKey::Wander(_) => "xon-wander",
        };
        Some((NavGoal::Position(d.goal_pos), d.replan))
    }

    /// Drive the injected navigator to `goal` — the canonical path-follow stage (q3's
    /// `locomote` shape): safe pursue → rate-limited yaw → arrive/creep throttle →
    /// traversal gates → stuck recovery → jump edges → traversal override LAST.
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
    ) -> (bool, f32) {
        nav.update(pos, None);
        nav.set_goal(goal, pos);
        if let Some(cm) = cm {
            nav.smooth_with_cm(cm, pos);
        }

        // Corner-cut-safe path look-ahead (Plan 48 L3): hull + lava-aware floor validation.
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

        // World move direction from the look-ahead; arrive + hazard creep (Plan 50).
        let world_dir = pursue_pt
            .map(|pt| {
                let d = pt - pos;
                Vec3::new(d.x, d.y, 0.0).normalize_or_zero()
            })
            .unwrap_or(Vec3::ZERO);
        let arrive = pursue_pt
            .map(|pt| Steering::arrive_scale((pt - pos).length()))
            .unwrap_or(1.0);
        let creep = crate::hazard::creep_scale(cm, pos, world_dir);
        let (fwd, side) = move_from_world_dir(world_dir, view_yaw, true);
        mv.move_forward(fwd * arrive * creep);
        mv.move_side(side * arrive * creep);

        // Low-skill overshoot stop (`havocbot.qc:1130-1134`): a clumsy mover at speed whose
        // velocity deviates > 70° from the desired direction slams the brakes for 0.4-0.6 s.
        if self.sk.movement() <= 3.0 {
            let vel = view.self_state().velocity.truncate();
            let speed = vel.length();
            if speed > 200.0 && world_dir.length_squared() > 0.5 {
                let vel_dir = (vel / speed).extend(0.0);
                if vel_dir.dot(world_dir) < (70f32).to_radians().cos() {
                    self.overshoot_until = self.time + 0.4 + self.rng.next() * 0.2;
                }
            }
        }
        if self.time < self.overshoot_until {
            mv.move_forward(0.0);
            mv.move_side(0.0);
        }

        // Traversal gates (Plan 46): swim/ride/ladder suspend stuck recovery + jump-edge.
        let gates = self.traverse.gates(nav, cm, pos, dt);

        // Stuck recovery (Plan 13/48).
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

        // Traversal override LAST (Plan 46): the shared executor owns swim/ride/ladder axes.
        let frame = TraversalFrame {
            view,
            cm,
            pos,
            view_yaw,
            steer_fwd: fwd,
            steer_side: side,
            dt,
        };
        let traversing = self.traverse.apply(mv, gates, nav, &frame).is_some();
        (
            traversing,
            pursue_pt.map(|pt| (pt - pos).length()).unwrap_or(0.0),
        )
    }
}

impl Brain for XonBrain {
    fn set_map(&mut self, map: BrainMap) {
        let BrainMap {
            roam_nodes,
            nav_graph,
            roam_as_position,
            items,
        } = map;
        self.roam_nodes = roam_nodes;
        self.nav_graph = Some(nav_graph);
        self.roam_as_position = roam_as_position;
        // The static item table feeds the rating sessions (values × ItemMemory availability).
        self.map_items = items;
    }

    fn status(&self) -> &str {
        self.status
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

        // PVS-honest item memory (evidence for goal expiry + candidate availability).
        self.item_memory.observe(&self.map_items, view, self.time);

        // ── Combat perception (T3): sticky enemy + weapon choice ───────────────────────
        let mut weapon_request = None;
        let enemy = if self.cfg.combat_enabled {
            self.enemy.tick(view, cm, self.time)
        } else {
            None
        };
        let held = view
            .self_state()
            .held_weapon
            .unwrap_or(crate::weapons::Weapon::Blaster);
        if let Some(e) = enemy {
            let dist = (e.pos - pos).length();
            weapon_request = self.weapon.tick(
                &self.sk,
                dist,
                held,
                view.self_state().held_ammo(),
                self.fired_at,
                self.time,
            );
        }

        let mut mv = MovementIntent::new();
        if let Some(nav) = nav {
            // Scenario / pinned-goal override always path-follows (the spawn-to-* contract);
            // otherwise the T2 rating-session strategy picks the goal.
            let goal = match goal_override.clone() {
                Some(g) => g,
                None => match self.strategy_goal(view, pos, dt) {
                    Some((g, replan)) => {
                        if replan {
                            nav.force_replan();
                        }
                        g
                    }
                    // Nothing ratable (no graph candidates yet) — interim roam ladder.
                    None => self.roam_goal(view, ticks, pos),
                },
            };
            let (traversing, pursue_dist) = self.locomote(nav, cm, pos, goal, dt, view, &mut mv);

            // Flight-path projectile dodge (T5): PVS rockets/grenades with fresh velocity.
            // Hazard-gated (Plan 48 L2): mirror a deadly dodge, cancel when both sides kill.
            let projectiles: Vec<(Vec3, Vec3)> = view
                .entities()
                .filter(|e| {
                    matches!(
                        e.class,
                        EntityClass::ProjectileRocket | EntityClass::ProjectileGrenade
                    ) && !e.is_stale
                })
                .map(|e| (e.origin, e.velocity.unwrap_or(Vec3::ZERO)))
                .collect();
            let mut dodge_vec =
                dodge::flight_path_dodge(pos, &projectiles, self.sk.dodge()).unwrap_or(Vec3::ZERO);
            if dodge_vec != Vec3::ZERO {
                if let Some(c) = cm {
                    if crate::hazard::dir_is_hazardous(c, pos, dodge_vec) {
                        dodge_vec = -dodge_vec;
                        if crate::hazard::dir_is_hazardous(c, pos, dodge_vec) {
                            dodge_vec = Vec3::ZERO;
                        }
                    }
                }
            }

            // ── Aim & fire (T4): the XonAim dynamical system owns the view while an enemy
            // is engaged — EXCEPT during traversal legs (the executor owns the view). Legs
            // are re-expressed against the aim yaw (the zb2 R2 lesson: never discard
            // recovery/steering legs while firing). ──────────────────────────────────────
            if let Some(e) = enemy {
                if !traversing {
                    let eye = los::eye_origin(pos.into());
                    let eye_v = Vec3::from(eye);
                    // Sight distance along the CURRENT view (one tick stale vs the vendor's
                    // post-turn trace — acceptable at 10 Hz).
                    let old_yaw = self.steering.view_yaw();
                    let sight_dist = cm
                        .map(|c| {
                            let f = crate::steer::view_forward(old_yaw);
                            let end = eye_v + f * 1000.0;
                            let t = c.trace(
                                &[eye_v.x, eye_v.y, eye_v.z],
                                &[end.x, end.y, end.z],
                                &[0.0; 3],
                                &[0.0; 3],
                                world::MASK_SOLID,
                            );
                            t.fraction * 1000.0
                        })
                        .unwrap_or(f32::INFINITY);
                    let inputs = AimInputs {
                        eye: eye_v,
                        target_pos: e.pos,
                        target_vel: e.vel.unwrap_or(Vec3::ZERO),
                        shot_speed: held.projectile_speed().unwrap_or(1_000_000.0),
                        // Fixed latency estimate (real RTT plumbing is a follow-up; since
                        // Plan 57 our real ping ≈ 16 ms + interp).
                        latency: 0.05,
                        fighting: true,
                        accurate: held.is_hitscan(),
                        sight_dist,
                    };
                    let current = Angles {
                        pitch: self.view_pitch,
                        yaw: old_yaw,
                    };
                    let cmd = self.aim.step(&mut self.rng, &self.sk, current, &inputs, dt);

                    // Re-express the locomotion legs against the aim yaw, with keepaway +
                    // the dodge folded in (the vendor composes `dir + dodge`, :1269-1278).
                    let mut legs_world = crate::steer::view_forward(old_yaw) * mv.forward
                        + crate::steer::view_right(old_yaw) * mv.side;
                    let dist = (e.pos - pos).length();
                    if dist < 80.0 {
                        // Keepaway (`havocbot.qc:915-931`): halt the approach 80 u out —
                        // strip the closing component, keep any lateral motion.
                        let to_enemy = (e.pos - pos).truncate().normalize_or_zero().extend(0.0);
                        let closing = legs_world.dot(to_enemy).max(0.0);
                        legs_world -= to_enemy * closing;
                    }
                    if dodge_vec != Vec3::ZERO {
                        legs_world = (legs_world + dodge_vec).normalize_or_zero()
                            * legs_world.length().max(dodge_vec.length());
                    }
                    let (ff, ss) = move_from_world_dir(legs_world, cmd.angles.yaw, false);
                    mv.look_at(cmd.angles.yaw, cmd.angles.pitch);
                    mv.move_forward(ff);
                    mv.move_side(ss);
                    self.steering.set_view_yaw(cmd.angles.yaw);
                    self.view_pitch = cmd.angles.pitch;

                    // Fire: cone-armed AND actually hittable (LOS) AND no self-splash.
                    if cmd.fire {
                        let los_ok = cm
                            .map(|c| los::has_los_player(c, eye, e.pos.into()))
                            .unwrap_or(true);
                        let splash = cm.is_some_and(|c| {
                            shared_aim::would_self_splash(c, eye_v, pos, e.pos, held)
                        });
                        if los_ok && !splash {
                            mv.attack();
                            self.fired_at = Some(self.time);
                        }
                    }
                }
            } else {
                // No enemy: pitch relaxes to level (locomote already looks flat).
                self.view_pitch = 0.0;
                if dodge_vec != Vec3::ZERO && !traversing {
                    // Dodge while roaming: slide off the flight line without turning.
                    let yaw = self.steering.view_yaw();
                    let legs_world = crate::steer::view_forward(yaw) * mv.forward
                        + crate::steer::view_right(yaw) * mv.side
                        + dodge_vec;
                    let (ff, ss) = move_from_world_dir(legs_world, yaw, false);
                    mv.move_forward(ff);
                    mv.move_side(ss);
                }
            }

            // Keyboard-emulation texture LAST (T5, `havocbot.qc:272-341`): quantize the
            // final legs at the skill-gated re-key cadence. Suspended during traversal
            // legs (swim/ride/ladder need analog precision).
            if !traversing {
                let (kf, ks) = self.keyboard.quantize(
                    &mut self.rng,
                    &self.sk,
                    (mv.forward, mv.side),
                    pursue_dist,
                    dt,
                );
                // Stale-key hazard veto (Plan 63 B4): quantize runs AFTER every hazard
                // gate and holds keys across ticks, so a held key can point into lava on
                // a tick where the (gated) analog legs no longer do. If the quantized
                // direction is hazardous, release the keys and keep the analog legs.
                let yaw = self.steering.view_yaw();
                let key_world =
                    crate::steer::view_forward(yaw) * kf + crate::steer::view_right(yaw) * ks;
                let key_hazard = key_world.length_squared() > 1e-4
                    && cm.is_some_and(|c| crate::hazard::dir_is_hazardous(c, pos, key_world));
                if key_hazard {
                    self.keyboard.release();
                } else {
                    mv.move_forward(kf);
                    mv.move_side(ks);
                }
            }
        } else {
            // No nav graph yet — walk forward so the bot isn't a statue.
            mv.move_forward(1.0);
        }

        // Survival override (Plan 50 E2): standing in lava/slime outranks everything.
        // Health-gated: a sinking corpse still ticks.
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
                let vel = view.self_state().velocity;
                tracing::debug!(
                    x = pos.x as i32,
                    y = pos.y as i32,
                    z = pos.z as i32,
                    vz = vel.z as i32,
                    hs = vel.truncate().length() as i32,
                    "EVT lava_escape"
                );
            }
        }

        BrainOutput {
            intent: mv,
            weapon_request,
            intent_forward: mv.forward,
        }
    }

    fn on_death(&mut self) {
        // Respawned elsewhere — steering/recovery state is stale; loadout back to Blaster.
        self.recovery.reset();
        self.enemy.reset();
        self.weapon.reset();
        self.fired_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav_mode::StubNav;
    use crate::perception::Worldview;
    use client::parse::ConfigStrings;
    use q2proto::Frame;

    fn view0() -> Worldview {
        Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0)
    }

    #[test]
    fn walks_toward_lookahead() {
        let mut b = XonBrain::new(XonSkill::default(), BrainConfig::default());
        let view = view0();
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
        assert!(nav.last_goal.is_some(), "a roam goal was set");
        assert!(out.intent.forward > 0.0, "walks toward the look-ahead");
        assert!(out.weapon_request.is_none(), "no combat before T3");
    }

    #[test]
    fn goal_override_drives_the_navigator() {
        let mut b = XonBrain::new(XonSkill::default(), BrainConfig::default());
        let view = view0();
        let mut nav = StubNav::default();
        let goal = NavGoal::Position(Vec3::new(7.0, 8.0, 9.0));
        let _ = b.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: Some(goal.clone()),
        });
        assert_eq!(
            nav.last_goal,
            Some(goal),
            "scenario contract: honor the override"
        );
    }

    /// Plan 60 T6: two brains with identical seeds + identical scripted inputs must emit
    /// byte-identical intent streams (the vendor's RNG is the only nondeterminism source,
    /// and ours is the seeded per-bot Lcg).
    #[test]
    fn two_seeded_runs_are_identical() {
        let run = || {
            let mut b = XonBrain::with_ordinal(XonSkill::default(), BrainConfig::default(), 42);
            let view = view0();
            let mut nav = StubNav {
                pursue: Some(Vec3::new(300.0, 150.0, 0.0)),
                ..Default::default()
            };
            let mut out = Vec::new();
            for t in 0..100 {
                let o = b.tick(BrainContext {
                    view: &view,
                    nav: Some(&mut nav),
                    cm: None,
                    dt: 0.1,
                    ticks: t,
                    goal_override: None,
                });
                out.push((
                    o.intent.forward.to_bits(),
                    o.intent.side.to_bits(),
                    o.intent.yaw.to_bits(),
                    o.intent.attack,
                ));
            }
            out
        };
        assert_eq!(run(), run(), "seeded runs must replay identically");
    }

    #[test]
    fn never_attacks_without_an_enemy() {
        // Empty PVS: the aim/fire stage must stay silent (no attack, level pitch).
        let mut b = XonBrain::new(XonSkill::default(), BrainConfig::default());
        let view = view0();
        let mut nav = StubNav {
            pursue: Some(Vec3::new(100.0, 0.0, 0.0)),
            ..Default::default()
        };
        for _ in 0..20 {
            let out = b.tick(BrainContext {
                view: &view,
                nav: Some(&mut nav),
                cm: None,
                dt: 0.1,
                ticks: 1,
                goal_override: None,
            });
            assert!(!out.intent.attack);
            assert_eq!(out.intent.pitch, 0.0);
        }
    }

    #[test]
    fn no_nav_walks_forward() {
        let mut b = XonBrain::new(XonSkill::default(), BrainConfig::default());
        let view = view0();
        let out = b.tick(BrainContext {
            view: &view,
            nav: None,
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: None,
        });
        assert!(out.intent.forward > 0.0);
        assert_eq!(b.status(), "xon");
    }
}
