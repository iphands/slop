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

use crate::brains::core::{BrainContext, BrainMap, MapItem};
use crate::combat::{CombatDecision, CombatDriver};
use crate::danger::DangerDriver;
use crate::fsm::{BehaviorIntent, BehaviorState};
use crate::move_ctrl::MovementIntent;
use crate::nav::NavGoal;
use crate::perception::EntityClass;
use crate::persona::Persona;
use crate::recover::{Recovery, RecoveryAction};
use crate::skill::BotSkill;
use crate::steer::{move_from_world_dir, Steering};
use crate::traverse::{TraversalExecutor, TraversalFrame};
use crate::weapons::Weapon;
use crate::{items, los, weapons};

// `BrainConfig`/`BrainOutput` live in `brains::core` next to the `trait Brain` contract;
// re-exported here for the convenience of code that reaches them via the `main` module.
pub use crate::brains::core::{BrainConfig, BrainOutput};

// Flee/kite health thresholds + kite distance are now persona-driven (Plan 27):
// `self.persona.flee_health()` (was 30), `.kite_health()` (was 50), `.kite_dist()` (was 450).
// The default persona reproduces those exact values, so this is behavior-preserving.

/// The `main` decision brain: owns combat/FSM/dodge/steering/recovery/skill/roam state.
pub struct MainBrain {
    skill: BotSkill,
    /// Per-bot personality (Plan 27) — the tactical thresholds (flee/kite health, kite distance,
    /// roam dwell) read from here instead of global consts. Default reproduces them exactly.
    persona: Persona,
    fsm: BehaviorState,
    combat: CombatDriver,
    danger: DangerDriver,
    steering: Steering,
    recovery: Recovery,
    /// The shared ladder/swim/ride executor (Plan 46). MainBrain previously had only a stateless
    /// ride + swim and NO ladder machine; delegating gains all three (and the stateful board lock).
    traverse: TraversalExecutor,
    /// Roam goal cursor (node indices into the A* graph) + position in it.
    roam_nodes: Vec<usize>,
    roam_idx: usize,
    /// The A* graph handle, kept so the navmesh backend can resolve a roam node index to
    /// a world position. `None` until the map loads.
    nav_graph: Option<Arc<NavGraph>>,
    /// `true` when the active nav backend (navmesh) cannot path to a bare node index, so
    /// roam goals are expressed as world positions instead. Set at map load.
    roam_as_position: bool,
    /// Static item spawns known from the map file (Plan 30) — for map-known resource seeking
    /// (health-when-hurt, ammo re-arm) beyond PVS. Populated at `set_map`.
    map_items: Vec<MapItem>,
    /// Per-bot memory of which map items are currently taken (Plan 30 T2), PVS-honest.
    item_memory: items::ItemMemory,
    /// Monotonic seconds since connect (accumulated from `dt`) — the clock for `item_memory`.
    time: f32,
    cfg: BrainConfig,
}

impl MainBrain {
    /// Construct a brain before the map is known. Roam goals + the graph handle are
    /// supplied later via [`set_map`](crate::brains::core::Brain::set_map) (mirrors how
    /// `bot_task` built its sub-drivers early and learned the nav graph at map load).
    pub fn new(skill: BotSkill, cfg: BrainConfig) -> Self {
        let steering = Steering::new(skill.combat());
        let persona = Persona::from_bot_skill(&skill);
        Self {
            skill,
            persona,
            fsm: BehaviorState::Roam,
            combat: CombatDriver::new(),
            danger: DangerDriver::new(),
            steering,
            recovery: Recovery::new(),
            traverse: TraversalExecutor::new(),
            roam_nodes: Vec::new(),
            roam_idx: 0,
            nav_graph: None,
            roam_as_position: false,
            map_items: Vec::new(),
            item_memory: items::ItemMemory::new(),
            time: 0.0,
            cfg,
        }
    }

    /// Override the persona (Plan 27 `--persona`); `None` keeps the skill-derived default.
    /// Builder-style so `build_brain` can apply a roster preset without a wider constructor.
    pub fn with_persona(mut self, persona: Option<Persona>) -> Self {
        if let Some(p) = persona {
            self.persona = p;
        }
        self
    }

    /// The current behavior state (typed). The public, FSM-agnostic label is the trait's
    /// [`status`](crate::brains::core::Brain::status); this stays only for the unit test that
    /// asserts the typed state.
    #[cfg(test)]
    pub(crate) fn behavior(&self) -> &BehaviorState {
        &self.fsm
    }

    /// Where to run while disengaging an unwinnable fight (Plan 45): grab the best resource
    /// (weapon when weak, health/armor when hurt) if one is visible, else step directly away
    /// from `enemy_pos`. Mirrors the Q3 brain's `retreat_goal` but uses `main`'s loadout-aware
    /// item picker.
    fn retreat_goal(
        &self,
        view: &crate::perception::Worldview,
        enemy_pos: Option<Vec3>,
    ) -> NavGoal {
        let ss = view.self_state();
        // Hurt → head for the nearest *reachable* known health/armor (Plan 30 T3), even if it's
        // outside PVS: the literal "collect health when hurt". A* path distance (not euclidean) so
        // a pack 200u away through a wall doesn't count as "near".
        if view.is_low_health() {
            if let Some(p) = self.nearest_reachable_item(
                view,
                &[EntityClass::ItemHealth, EntityClass::ItemArmor],
                FLEE_HEALTH_MAX_ASTAR,
            ) {
                return NavGoal::Position(p);
            }
        }
        // Else fall back to the PVS-visible loadout-aware picker (weapon when weak, health/armor
        // when hurt but nothing map-known reachable).
        if let Some((p, _)) =
            items::best_item_goal_weighted(view, &self.skill, ss.held_weapon, ss.health, ss.armor)
        {
            return NavGoal::Position(p);
        }
        let pos = ss.origin;
        let away = enemy_pos
            .map(|e| {
                let a = pos - e;
                Vec3::new(a.x, a.y, 0.0).normalize_or_zero()
            })
            .unwrap_or(Vec3::X);
        NavGoal::Position(pos + away * 300.0)
    }

    /// The world origin of the nearest **reachable** map-known item whose class is in `classes`
    /// and which `item_memory` believes is available (Plan 30 T3). "Reachable/near" is measured by
    /// **A\* path length** through the nav graph, not euclidean distance, and is capped at
    /// `max_astar` — a hurt bot grabs *nearby* health but must NOT sprint across the map mid-fight
    /// (the unbounded version halved combat activity in the q2dm1 A/B, 2026-07-10). Bounded per tick
    /// by an euclidean prefilter to the closest [`ITEM_ASTAR_CANDIDATES`] candidates. `None` if the
    /// graph isn't loaded or nothing reachable within `max_astar` qualifies.
    fn nearest_reachable_item(
        &self,
        view: &crate::perception::Worldview,
        classes: &[EntityClass],
        max_astar: f32,
    ) -> Option<Vec3> {
        let graph = self.nav_graph.as_ref()?;
        let pos = view.self_state().origin;
        let from = graph.nearest(&[pos.x, pos.y, pos.z])?;

        // Collect available candidates of the wanted classes, with a resolved nav node.
        let mut cands: Vec<(f32, &MapItem, usize)> = self
            .map_items
            .iter()
            .enumerate()
            .filter(|(_, it)| classes.contains(&it.class))
            .filter(|(i, it)| self.item_memory.available(*i, it.class, self.time))
            .filter_map(|(_, it)| it.nav_node.map(|n| ((it.origin - pos).length(), it, n)))
            .collect();
        // Euclidean prefilter → cap the A* work.
        cands.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        cands.truncate(ITEM_ASTAR_CANDIDATES);

        // Pick the one with the shortest actual A* path — but only if it is within `max_astar`
        // (don't abandon the fight to chase a pack on the far side of the map).
        cands
            .iter()
            .filter_map(|(_, it, node)| {
                graph
                    .path(from, *node)
                    .map(|p| (graph.path_len(&p), it.origin))
            })
            .filter(|(len, _)| *len <= max_astar)
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, origin)| origin)
    }
}

/// Max A* path length (units) a hurt `main` will divert to grab a known health/armor pack (Plan 30
/// T3). Roughly a few seconds' run — beyond this the bot keeps fighting/kiting rather than sprint
/// across the map (the unbounded seek halved combat activity in the 2026-07-10 q2dm1 A/B).
const FLEE_HEALTH_MAX_ASTAR: f32 = 900.0;

/// Cap on how many map-item candidates get an A* path scored per tick (euclidean-nearest first),
/// so the health-seek stays cheap on large graphs (Plan 30 T3 Risk #1).
const ITEM_ASTAR_CANDIDATES: usize = 8;

impl crate::brains::core::Brain for MainBrain {
    /// Supply the per-map roam goals + A* graph handle once the map has loaded.
    /// `roam_as_position` is `true` for backends (navmesh) that path to world positions
    /// rather than bare node indices.
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
        self.map_items = items;
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
        // Advance the item-memory clock and observe which map-item pads are stocked/empty this
        // frame (Plan 30 T2/T3) — PVS-honest, per-bot.
        self.time += dt;
        self.item_memory.observe(&self.map_items, view, self.time);
        let jitter = (ticks as f32) * 0.1;
        let combat_dec = if self.cfg.combat_enabled {
            self.combat.evaluate(view, &self.skill, jitter, cm)
        } else {
            CombatDecision::default()
        };

        // ── Underpowered handling (Plan 45) ──────────────────────────────────
        // Obituaries showed the losing loop: `main` dies → respawns with only the spawn
        // Blaster → loses the projectile duel to `q3` (whose aim + dodge win it) → dies →
        // repeat. So loadout, not just health, gates our posture:
        //   • Blaster (or nothing) → we're out-gunned: fight *evasively* (kite — a moving
        //     target survives the duel we'd lose standing still) and, if a real gun is in
        //     reach, *disengage to grab it* (`flee_hard`). Arming up is the whole game.
        //   • A real (hitscan) weapon → `main`'s aim is near-perfect: stand and delete.
        // A full run-to-nowhere retreat measured worse (shot in the back), so `flee_hard`
        // only fires when near death or when an actual weapon pickup is visible to run to.
        let self_ss = view.self_state();
        let health = self_ss.health;
        let held = self_ss.held_weapon;
        let has_target = combat_dec.target_entity.is_some();
        // Stuck on the near-useless spawn Blaster? (No resolved weapon counts the same.)
        let blaster_only = matches!(held, None | Some(Weapon::Blaster));
        // A weapon pickup we could grab to escape the Blaster phase (weighted picker puts
        // weapons first while we hold the Blaster).
        let weapon_in_reach = matches!(
            items::best_item_goal_weighted(view, &self.skill, held, health, self_ss.armor),
            Some((_, crate::perception::EntityClass::ItemWeapon))
        );
        let flee_hard = self.cfg.combat_enabled
            && has_target
            && (health < self.persona.flee_health() || (blaster_only && weapon_in_reach));
        let kite = self.cfg.combat_enabled
            && has_target
            && !flee_hard
            && (health < self.persona.kite_health() || blaster_only);
        let enemy_pos_now: Option<Vec3> = combat_dec.target_entity.and_then(|t| {
            view.entities()
                .find(|e| e.entity_number == t)
                .map(|e| e.origin)
        });

        // Pass combat target to FSM for navigation goal.
        // Only chase via nav when LOS holds (Plan 11 T4) — without
        // LOS the bot was walking into walls toward walled enemies.
        let fsm_intent = if flee_hard {
            // Near death: break off and sprint to the best resource (health/weapon/armor).
            if !matches!(self.fsm, BehaviorState::Flee) {
                tracing::debug!(target = ?combat_dec.target_entity, health, "critical — flee");
            }
            self.fsm = BehaviorState::Flee;
            BehaviorIntent {
                nav_goal: Some(self.retreat_goal(view, enemy_pos_now)),
                should_pickup: None,
            }
        } else if let Some(target) = combat_dec.target_entity {
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
            } else if let Some((item_pos, _)) = items::best_item_goal_weighted(
                view,
                &self.skill,
                view.self_state().held_weapon,
                view.self_state().health,
                view.self_state().armor,
            ) {
                // Seek the highest-value visible item (powerups, armor, weapons)
                // weighted by value/distance and — for `main` (Plan 45) — by loadout
                // need (weapon hunger when weak) and health/armor need when hurt.
                NavGoal::Position(item_pos)
            } else if !self.roam_nodes.is_empty() {
                // Campers dwell ~5x longer per node (first-cut
                // camping; a true camp-node picker with cover/LOS
                // is a follow-up). Default roamer cycles every 5s.
                let dwell = self.persona.roam_dwell();
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

            // Per-weapon ideal engagement band (Plan 28 T2): hold the range where OUR held weapon
            // wins — a shotgunner rushes, a railgunner holds out, a rocketeer stays outside its
            // splash. Replaces the old fixed `ideal_dist=160`/`backup_dist=80` for all weapons.
            // Unknown weapon (early frames) → the historical default band so behavior is preserved.
            let band = held
                .map(weapons::ideal_range)
                .unwrap_or(weapons::RangeBand {
                    backup: 80.0,
                    ideal: 160.0,
                });
            let ideal_dist = band.ideal;
            let backup_dist = band.backup;

            // Resolve enemy position + distance (if we have a target in view). While
            // hard-fleeing (Plan 45) we treat the enemy as absent for the distance-band
            // movement logic so the bot path-follows its escape route instead of holding
            // ideal combat distance — the dedicated flee branch below drives the move.
            // (Kiting keeps the enemy here: it needs the distance/direction to back off.)
            let enemy_dist_dir: Option<(f32, Vec3)> = if flee_hard {
                None
            } else {
                combat_dec.target_entity.and_then(|t| {
                    view.entities().find(|e| e.entity_number == t).map(|enemy| {
                        let to = enemy.origin - pos;
                        let d = to.length();
                        let dir = if d > 1.0 { to / d } else { Vec3::X };
                        (d, dir)
                    })
                })
            };

            // ── 1. Ideal view yaw (priority: fire-aim > enemy-face > path) ──
            let (ideal_yaw, ideal_pitch) = if combat_dec.should_fire {
                (combat_dec.aim_yaw, combat_dec.aim_pitch)
            } else if let Some((d, dir)) = enemy_dist_dir {
                if d < ideal_dist {
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

            let (world_move_dir, face_then_go) = if flee_hard {
                // Hard flee (Plan 45): move along the escape path (toward the resource /
                // away node). If we're firing (view is locked on the enemy behind us) use
                // raw decomposition (`face_then_go = false`) so `forward` can go negative
                // and we backpedal while shooting; otherwise face the escape heading and run.
                let escape = nav
                    .pursue_target(pos)
                    .map(|pt| {
                        let d = pt - pos;
                        Vec3::new(d.x, d.y, 0.0).normalize_or_zero()
                    })
                    .unwrap_or_else(|| {
                        enemy_pos_now
                            .map(|e| {
                                let a = pos - e;
                                Vec3::new(a.x, a.y, 0.0).normalize_or_zero()
                            })
                            .unwrap_or(Vec3::ZERO)
                    });
                (escape, !combat_dec.should_fire)
            } else if let Some((d, dir)) = enemy_dist_dir {
                if kite {
                    // Kite (Plan 45): out-gunned but viable — keep facing + firing (aim/yaw
                    // still lock the enemy) while opening range. Back away until the persona's kite
                    // distance, then hold and strafe. `face_then_go = false` so we backpedal facing.
                    let tan = Vec3::new(-dir.y, dir.x, 0.0) * self.steering.strafe_tick(dt);
                    if d < self.persona.kite_dist() {
                        let away = Vec3::new(-dir.x, -dir.y, 0.0).normalize_or_zero();
                        ((away + tan * 0.6).normalize_or_zero(), false)
                    } else {
                        (tan.normalize_or_zero() * 0.7, false)
                    }
                } else if d < backup_dist {
                    // Back away from enemy while keeping aim on them.
                    let away = Vec3::new(-dir.x, -dir.y, 0.0).normalize_or_zero();
                    // Add tangential even while backing (keeps bot moving).
                    let tan = Vec3::new(-dir.y, dir.x, 0.0)
                        * self.steering.strafe_tick(dt)
                        * strafe_weight;
                    ((away + tan).normalize_or_zero(), false)
                } else if d < ideal_dist {
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

            // Traversal gates (Plan 46): the shared TraversalExecutor tells us whether we're
            // swimming (Plan 40) or riding a platform/lift/ladder (Plan 43/35). Both SUSPEND stuck
            // recovery — a surface bob or a stand-and-wait on a lift is not a wedge (the
            // StuckDetector false-fires and find_best_direction steers AWAY from water) — and keep
            // the ride LOCKED active while boarded. MainBrain now ALSO gets ladder climbs + the
            // stateful board/carry lock it previously lacked; the swim/ride override is applied
            // below (after normal steering) via `self.traverse.apply`.
            let gates = self.traverse.gates(nav, cm, pos);

            // ── 6. Stuck recovery (Plan 13) ───────────────────────────────
            let has_nav_target = nav.pursue_target(pos).is_some();
            let engaging = matches!(self.fsm, BehaviorState::Engage { .. });
            let rec_action = if gates.any() {
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

            // ── 7. Jump-edge activation (Plan 14 T2) — suspended while traversing ─────
            if nav.current_edge_is_jump() && !gates.any() {
                mv.jump();
            }

            // ── 8. Traversal override (Plan 46): swim (Plan 40) + stateful ride/ladder (Plan
            // 43/35) movement is owned by the shared TraversalExecutor. On a swim/ride/ladder edge
            // it OVERWRITES the movement axes (and view) computed above; the fire decision stays
            // with combat (the bot fires along the traversal heading — accepted for v1). `fwd`/
            // `side` are the raw (pre-arrive) steering the swim machine reuses.
            let frame = TraversalFrame {
                view,
                cm,
                pos,
                view_yaw,
                steer_fwd: fwd,
                steer_side: side,
            };
            self.traverse.apply(&mut mv, gates, nav, &frame);
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
