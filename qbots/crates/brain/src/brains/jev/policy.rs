//! # brain::brains::jev::policy — `JevPolicy`, the `GoalPolicy` the jev brain plugs into q3
//!
//! It owns the bot's `DecisionSource` handle, copies of the map facts q3 discards (`items`) or
//! keeps privately (`roam_nodes`, `nav_graph`), the **inert** flag, and the **apply path**:
//!
//! - a **ticket ring** (the last [`RING`] asks, each with the options it offered, in order);
//! - once per tick, a *new* decision from `latest()` is resolved **only against its own
//!   ticket** — never against the list the brain would build now — and acked exactly once:
//!   `Applied`, or `Rejected(Inert | WrongMap | UnknownTicket | OffTicketLabel)`;
//! - an accepted decision becomes the policy's [`current`](JevPolicy::current) decision.
//!
//! **Posture is the first live question** (Plan 72 W2: the objective failed calibration and is
//! not asked). Once per tick, from `JevBrain::tick` ([`pre_tick`](JevPolicy::pre_tick)):
//! - with an enemy in view, the policy **asks** `brain::jev::question::posture_question()` on
//!   `Contact` (an enemy appears), `Damage` (hit this tick) or a due `Refresh` — the hub's
//!   per-bot pacing decides which asks go out;
//! - an applied `disengage` biases q3 through T6's merge (retreat **and** no chase) for
//!   [`POSTURE_MAX_AGE_S`] of brain time; `hold` is the no-op. Death, a level change and a newer
//!   answered ticket release it.
//!
//! **The pickup objective (Plan 74 T4)** — asked only with **no** enemy in view, at least
//! [`RESPAWN_SETTLE_S`] after a respawn, on a pending event (`Respawn`, `MapChange`, `Released`)
//! or a due objective refresh, and never while an objective is held:
//! - candidates come from [`pads::candidates`] (one graph flood per ask); the ask's state gains
//!   the `pads` field and offers `objective_question_v2` over the classes that have a pad;
//! - an answer resolves through **its own** ticket's pad list (`carry_on` → no goal); an applied
//!   seek is **held** — q3's roam fallback walks to the pad — until arrival (≤ [`ARRIVE_RADIUS`]
//!   horizontally), the pad seen empty, [`OBJECTIVE_MAX_S`], `on_stall`, death, a map change, or
//!   a newer ticket that **asked the objective** (a posture-only ticket never releases it);
//! - a release sets the `Released` trigger, so the next ask follows promptly.
//!
//! While inert (`goal_override` set — the `spawn-to-*` harnesses) nothing is asked, a new
//! decision is acked `Inert`, and the posture stays q3's own.

use std::collections::VecDeque;
use std::sync::Arc;

use world::NavGraph;

use super::pads::{self, Pad, PadCandidates};
use crate::brains::core::MapItem;
use crate::brains::q3::{GoalPolicy, GoalQuery, Posture, PostureQuery};
use crate::items::ItemMemory;
use crate::jev::question::{objective_question_v2, posture_question, QuestionKey};
use crate::jev::source::{
    AppliedDecision, ApplyOutcome, Ask, BrainReject, DecisionSource, ObjectiveOutcome, Outcome,
    PostureChoice, TicketId, Trigger, OBJECTIVE_WINDOW_S, OUTCOME_WINDOW_S,
};
use crate::jev::state::{DecisionState, Field, MapContext, MapId, SelfSummary};
use crate::nav::NavGoal;
use crate::perception::Worldview;

/// How long an applied `disengage` biases q3 (brain time, seconds) — about one fight.
pub const POSTURE_MAX_AGE_S: f32 = 6.0;
/// The window `recent` damage is summed over (brain time, seconds).
const DAMAGE_WINDOW_S: f32 = 10.0;

/// How many recent asks the brain can still resolve an answer against.
pub const RING: usize = 4;

/// An applied pickup objective is released after this long (brain time, seconds).
pub const OBJECTIVE_MAX_S: f32 = 30.0;
/// Within this horizontal distance of the pad, the objective has arrived.
pub const ARRIVE_RADIUS: f32 = 64.0;
/// No objective ask until the bot has been alive this long (brain time, seconds).
pub const RESPAWN_SETTLE_S: f32 = 2.0;
/// jev trusts a missing pad entity as "taken" only this close. Plan 74 B2: the shared 500 u
/// marked pads behind walls taken (0 % of B1's armor trips arrived); B3: at 192 u, 7 trips
/// released `empty` still reached the pad and picked the weapon up (a pad on another floor
/// level is outside PVS) — so only "standing next to it and it is not there" counts.
const PAD_TRUST_RANGE: f32 = 128.0;
/// A cached retreat pad is recomputed after this long (brain time, seconds) — each is one flood.
const RETREAT_REFRESH_S: f32 = 1.0;
/// After an objective ask attempt (sent, paced, or nothing to offer), wait this long before the
/// next — each attempt floods the nav graph once.
const OBJECTIVE_RETRY_S: f32 = 1.0;

/// What one ask offered, in offered order — the only list its answer may resolve against.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TicketEntry {
    /// The objective options' pads (index = the answer's `objective`; `None` = `carry_on`).
    /// Empty when the ask did not carry the objective question.
    pub objectives: Vec<Option<Pad>>,
    /// The objective options' labels, same order (journalled with the outcome).
    pub labels: Vec<&'static str>,
    /// The code rule's label at ask time — applied only by `jev_rule`, journalled for both.
    pub rule: Option<&'static str>,
    /// The posture options (index = the answer's `posture`).
    pub postures: Vec<PostureChoice>,
}

/// An accepted decision, resolved through its ticket.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    /// The ask it answers.
    pub ticket: TicketId,
    /// The chosen objective's pad, if that question was answered decisively with a seek.
    pub objective: Option<Pad>,
    /// The chosen posture, if that question was answered decisively.
    pub posture: Option<PostureChoice>,
}

/// The jev brain's policy. See the module doc.
pub struct JevPolicy {
    source: Arc<dyn DecisionSource>,
    inert: bool,
    roam_nodes: Vec<usize>,
    nav_graph: Option<Arc<NavGraph>>,
    items: Vec<MapItem>,
    /// Pads seen empty (Plan 74 T3), observed every tick — keyed by index into `items`.
    memory: ItemMemory,
    /// The applied pickup objective being walked to, and since when (Plan 74 T4).
    objective: Option<(Pad, f32)>,
    /// Does this policy ask the objective question? (`false` for the posture-only shadow.)
    asks_objective: bool,
    /// `jvr` (Plan 74 T5): the objective comes from the code rule; the model's is journalled.
    rule: bool,
    /// Brain time of the first tick of this life.
    alive_since: Option<f32>,
    /// No objective ask attempt before this brain time.
    next_objective_try: f32,
    map: MapId,
    ring: VecDeque<(TicketId, TicketEntry)>,
    last_seen: Option<TicketId>,
    current: Option<Resolved>,
    pending_trigger: Option<Trigger>,
    /// Brain time (Σ dt), for the posture lifetime and the damage window.
    time: f32,
    /// When the current decision was applied (brain time).
    applied_at: f32,
    last_health: Option<i32>,
    /// `(time, amount)` of recent hits.
    damage: VecDeque<(f32, u32)>,
    saw_enemy: bool,
    /// Shadow (Plan 73): ask, resolve and journal, but never steer q3.
    shadow: bool,
    /// Latest (health, armor, frags) seen — the outcome window's readings.
    vitals: (i32, i32, i32),
    /// The open outcome window, if a decision is being followed (Plan 73 journal).
    window: Option<Window>,
    /// The open **objective** outcome window (Plan 74 T7) — separate, so posture windows keep
    /// Plan 73's semantics.
    obj_window: Option<ObjWindow>,
    /// The held weapon's power tier, last tick (0 when unrecognised).
    held_tier: u8,
    /// Plan 74 B2: the cached retreat pad and when it was computed (one flood per second).
    retreat: Option<(f32, Option<glam::Vec3>)>,
    /// Retreat toward pads at all (off for the posture-only shadow).
    pad_retreat: bool,
    /// Per-node rim surcharge for pricing pads (Plan 74 B7), built once per map from the
    /// collision model the brain's tick carries; empty until then (plain travel cost).
    rim: Vec<f32>,
    /// What pads are priced with: `rim` plus the mover surcharge (Plan 74 B8).
    risk: Vec<f32>,
}

/// An open objective outcome window.
struct ObjWindow {
    ticket: TicketId,
    opened: f32,
    vitals: (i32, i32, i32),
    damage: u32,
    tier: u8,
    /// Where the applied objective leads, if anything was applied.
    target: Option<glam::Vec3>,
    applied: Option<&'static str>,
    rule: Option<&'static str>,
    arrived_at: Option<f32>,
    /// Why the objective was released inside the window (first release only).
    released: Option<&'static str>,
}

/// An open outcome window: the readings when it opened, and what has happened since.
struct Window {
    ticket: TicketId,
    opened: f32,
    vitals: (i32, i32, i32),
    damage: u32,
    q3_retreat: Option<bool>,
    biased: bool,
}

impl JevPolicy {
    /// A policy that reads from and asks `source`.
    pub fn new(source: Arc<dyn DecisionSource>) -> Self {
        Self {
            source,
            inert: false,
            roam_nodes: Vec::new(),
            nav_graph: None,
            items: Vec::new(),
            memory: ItemMemory::with_trust_range(PAD_TRUST_RANGE),
            objective: None,
            asks_objective: true,
            rule: false,
            alive_since: None,
            next_objective_try: 0.0,
            map: MapId::Other,
            ring: VecDeque::with_capacity(RING),
            last_seen: None,
            current: None,
            pending_trigger: None,
            time: 0.0,
            applied_at: 0.0,
            last_health: None,
            damage: VecDeque::new(),
            saw_enemy: false,
            vitals: (0, 0, 0),
            window: None,
            obj_window: None,
            held_tier: 0,
            retreat: None,
            pad_retreat: true,
            rim: Vec::new(),
            risk: Vec::new(),
            shadow: false,
        }
    }

    /// A shadow policy: asks and journals like jev, never biases q3 (the arena's control).
    pub fn new_shadow(source: Arc<dyn DecisionSource>) -> Self {
        Self {
            shadow: true,
            asks_objective: false,
            pad_retreat: false,
            ..Self::new(source)
        }
    }

    /// The `jvr` policy: objective by [`PadCandidates::rule_pick`], posture as jev.
    pub fn new_rule(source: Arc<dyn DecisionSource>) -> Self {
        Self {
            rule: true,
            ..Self::new(source)
        }
    }

    /// Is this the `jvr` rule policy?
    pub fn is_rule(&self) -> bool {
        self.rule
    }

    /// Is this a shadow policy?
    pub fn is_shadow(&self) -> bool {
        self.shadow
    }

    /// Close the open outcome window (if any) and report it.
    fn close_window(&mut self, died: bool) {
        let Some(w) = self.window.take() else {
            return;
        };
        let (hp, ar, fr) = self.vitals;
        self.source.outcome(
            w.ticket,
            Outcome {
                secs: self.time - w.opened,
                died,
                hp_delta: hp - w.vitals.0,
                armor_delta: ar - w.vitals.1,
                frags_delta: fr - w.vitals.2,
                damage_taken: w.damage,
                q3_retreat: w.q3_retreat,
                biased: w.biased,
                objective: None,
            },
        );
    }

    /// Close the open objective window (if any) and report it.
    fn close_obj_window(&mut self, died: bool) {
        let Some(w) = self.obj_window.take() else {
            return;
        };
        let (hp, ar, fr) = self.vitals;
        self.source.outcome(
            w.ticket,
            Outcome {
                secs: self.time - w.opened,
                died,
                hp_delta: hp - w.vitals.0,
                armor_delta: ar - w.vitals.1,
                frags_delta: fr - w.vitals.2,
                damage_taken: w.damage,
                q3_retreat: None,
                biased: w.target.is_some(),
                objective: Some(ObjectiveOutcome {
                    applied: w.applied,
                    rule: w.rule,
                    arrived: w.arrived_at.is_some(),
                    secs_to_arrive: w.arrived_at.map(|t| t - w.opened),
                    upgraded: self.held_tier > w.tier,
                    released: w.released,
                }),
            },
        );
    }

    /// Advance the objective window: damage, arrival, and its time limit.
    fn track_obj_window(&mut self, view: &Worldview, hit: u32) {
        let Some(w) = &mut self.obj_window else {
            return;
        };
        w.damage += hit;
        let here = view.self_state().origin;
        if w.arrived_at.is_none()
            && w.target
                .is_some_and(|t| (t - here).truncate().length() <= ARRIVE_RADIUS)
        {
            w.arrived_at = Some(self.time);
        }
        if self.time - w.opened >= OBJECTIVE_WINDOW_S {
            self.close_obj_window(false);
        }
    }

    /// Once per tick, before `poll` and before q3 runs: advance brain time, track damage, and
    /// ask the posture question when a fight warrants it (see the module doc).
    pub fn pre_tick(&mut self, view: &Worldview, dt: f32) {
        self.time += dt;
        let hp = view.self_state().health;
        let hit = self.last_health.map_or(0, |prev| (prev - hp).max(0) as u32);
        self.last_health = Some(hp);
        let s = view.self_state();
        self.vitals = (hp, s.armor, s.frags);
        self.held_tier = s.held_weapon.map_or(0, crate::weapons::Weapon::power_tier);
        self.track_obj_window(view, hit);
        if hit > 0 {
            self.damage.push_back((self.time, hit));
        }
        if let Some(w) = &mut self.window {
            w.damage += hit;
            if self.time - w.opened >= OUTCOME_WINDOW_S {
                self.close_window(false);
            }
        }
        while self
            .damage
            .front()
            .is_some_and(|(t, _)| self.time - t > DAMAGE_WINDOW_S)
        {
            self.damage.pop_front();
        }
        self.memory.observe(&self.items, view, self.time);
        let alive_since = *self.alive_since.get_or_insert(self.time);
        self.check_release(view);
        let enemy = view.enemies().any(|e| !e.is_stale);
        let contact = enemy && !self.saw_enemy;
        self.saw_enemy = enemy;
        if self.inert {
            return;
        }
        if enemy {
            let trig = if contact {
                Trigger::Contact
            } else if hit > 0 {
                Trigger::Damage
            } else if self.source.refresh_due(QuestionKey::Posture) {
                Trigger::Refresh
            } else {
                return;
            };
            self.ask(view, trig);
        } else if self.time - alive_since >= RESPAWN_SETTLE_S {
            self.maybe_ask_objective(view);
        }
    }

    /// The closed state for an ask from this frame.
    fn state(&self, view: &Worldview) -> DecisionState {
        let me = SelfSummary {
            recent_damage: self.damage.iter().map(|(_, n)| n).sum(),
            secs_since_damage: self.damage.back().map(|(t, _)| self.time - t),
        };
        let ctx = MapContext {
            map: self.map,
            nav_graph: self.nav_graph.as_deref(),
        };
        DecisionState::build(view, &ctx, &me)
    }

    /// Ask the pickup objective if it is wanted, allowed and has something to offer.
    fn maybe_ask_objective(&mut self, view: &Worldview) {
        if !self.asks_objective || self.objective.is_some() || self.time < self.next_objective_try {
            return;
        }
        let trig = match self.pending_trigger {
            Some(t) => t,
            None if self.source.refresh_due(QuestionKey::Objective) => Trigger::Refresh,
            None => return,
        };
        self.next_objective_try = self.time + OBJECTIVE_RETRY_S;
        let cands = self.pad_candidates(view);
        if cands.is_empty() {
            return;
        }
        let ask = Ask {
            state: self.state(view).with(Field::Pads(cands.facts())),
            questions: vec![objective_question_v2(cands.offered())],
            trig,
        };
        if let Some(ticket) = self.source.submit(ask) {
            self.pending_trigger = None;
            let s = view.self_state();
            // The rule's label is journalled for every objective ask (P2: does the model agree?);
            // only jvr applies it.
            let rule = Some(cands.rule_label(s.held_weapon, s.health, s.armor));
            if self.rule {
                // Same moment, same candidates as a jev ask; only the chooser differs.
                let pick = cands.rule_pick(s.held_weapon, s.health, s.armor);
                self.objective = pick.map(|p| (p, self.time));
            }
            self.record_ticket(
                ticket,
                TicketEntry {
                    objectives: cands.option_pads(),
                    labels: cands.offered().labels(),
                    rule,
                    postures: Vec::new(),
                },
            );
        }
    }

    /// Release the held objective on arrival, an empty pad, or old age.
    fn check_release(&mut self, view: &Worldview) {
        let Some((pad, since)) = self.objective else {
            return;
        };
        let here = view.self_state().origin;
        let arrived = (pad.origin - here).truncate().length() <= ARRIVE_RADIUS;
        let empty = self
            .items
            .get(pad.item)
            .is_none_or(|it| !self.memory.available(pad.item, it.class, self.time));
        let why = if arrived {
            "arrived"
        } else if empty {
            "empty"
        } else if self.time - since >= OBJECTIVE_MAX_S {
            "timeout"
        } else {
            return;
        };
        self.release_objective(why);
    }

    /// Drop the held objective and ask again soon; the open objective window records why.
    fn release_objective(&mut self, why: &'static str) {
        if self.objective.take().is_some() {
            self.pending_trigger.get_or_insert(Trigger::Released);
            if let Some(w) = self.obj_window.as_mut().filter(|w| w.released.is_none()) {
                w.released = Some(why);
            }
        }
    }

    /// Is `pos` at a deadly rim? The rim surcharge on the nearest graph node (`false` before
    /// it is built). Linear nearest-node scan — called only when a disengage is in force.
    fn at_rim(&self, pos: glam::Vec3) -> bool {
        let Some(g) = &self.nav_graph else {
            return false;
        };
        g.nearest(&pos.into())
            .and_then(|n| self.rim.get(n))
            .is_some_and(|c| *c > 0.0)
    }

    /// Install a rim overlay directly (tests; production builds it with [`Self::ensure_rim`]).
    #[cfg(test)]
    pub(crate) fn set_rim(&mut self, rim: Vec<f32>) {
        self.risk = rim.clone();
        self.rim = rim;
    }

    /// The risk overlay pads are priced with (empty ⇒ plain travel cost).
    fn risk(&self) -> &[f32] {
        &self.risk
    }

    /// Build the rim surcharge for the current map once a collision model is available
    /// (Plan 74 B7 — one probe pass per map, ~20 ms on q2dm3).
    pub fn ensure_rim(&mut self, cm: &world::CollisionModel) {
        if self.rim.is_empty() {
            if let Some(g) = &self.nav_graph {
                self.rim = crate::hazard::rim_node_costs(g, cm);
                // Lava rims (B7) and mover rides (B8) both make a pad route risky.
                self.risk = self
                    .rim
                    .iter()
                    .zip(pads::mover_node_costs(g))
                    .map(|(r, m)| r + m)
                    .collect();
            }
        }
    }

    /// The pickup objective being walked to, if any.
    pub fn held_objective(&self) -> Option<Pad> {
        self.objective.map(|(p, _)| p)
    }

    /// Build the closed state, ask the posture question, and remember what it offered.
    fn ask(&mut self, view: &Worldview, trig: Trigger) {
        let ask = Ask {
            state: self.state(view),
            questions: vec![posture_question()],
            trig,
        };
        if let Some(ticket) = self.source.submit(ask) {
            // Same order as `posture_question()`'s options.
            let postures = vec![PostureChoice::Disengage, PostureChoice::Hold];
            self.record_ticket(
                ticket,
                TicketEntry {
                    postures,
                    ..TicketEntry::default()
                },
            );
        }
    }

    /// Set per tick by `JevBrain` from `goal_override.is_some()`.
    pub fn set_inert(&mut self, inert: bool) {
        self.inert = inert;
    }

    /// Is the policy inert this tick?
    pub fn is_inert(&self) -> bool {
        self.inert
    }

    /// Keep copies of the map facts candidate generation needs (`JevBrain::set_map`).
    pub fn set_map_facts(
        &mut self,
        roam_nodes: Vec<usize>,
        nav_graph: Arc<NavGraph>,
        items: Vec<MapItem>,
    ) {
        self.roam_nodes = roam_nodes;
        self.nav_graph = Some(nav_graph);
        self.items = items;
        // Indices are into the old map's table.
        self.memory = ItemMemory::with_trust_range(PAD_TRUST_RANGE);
        self.objective = None;
        self.rim.clear();
        self.risk.clear();
    }

    /// The best pad per class for this bot now (Plan 74 T3) — one graph flood, so call it per
    /// ask, never per tick. Empty before the map loads.
    pub fn pad_candidates(&self, view: &Worldview) -> PadCandidates {
        match &self.nav_graph {
            Some(g) => pads::candidates(&self.items, &self.memory, g, self.risk(), view, self.time),
            None => PadCandidates::default(),
        }
    }

    /// Static item spawns of the current map (q3 itself discards these).
    pub fn items(&self) -> &[MapItem] {
        &self.items
    }

    /// Roam nodes of the current map.
    pub fn roam_nodes(&self) -> &[usize] {
        &self.roam_nodes
    }

    /// The current map's nav graph, once set.
    pub fn nav_graph(&self) -> Option<&Arc<NavGraph>> {
        self.nav_graph.as_ref()
    }

    /// The bot's decision source.
    pub fn source(&self) -> &Arc<dyn DecisionSource> {
        &self.source
    }

    /// The current map (closed identity), as last reported by `on_map_change`.
    pub fn map(&self) -> MapId {
        self.map
    }

    /// Remember what an accepted ask offered. Called by the ask path (T10) with the ticket
    /// `submit` returned; the oldest entry falls out past [`RING`].
    pub fn record_ticket(&mut self, ticket: TicketId, entry: TicketEntry) {
        if self.ring.len() == RING {
            self.ring.pop_front();
        }
        self.ring.push_back((ticket, entry));
    }

    /// The accepted decision currently in force, if any.
    pub fn current(&self) -> Option<&Resolved> {
        self.current.as_ref()
    }

    /// The event waiting to trigger the next ask (set by death / level change).
    pub fn pending_trigger(&self) -> Option<Trigger> {
        self.pending_trigger
    }

    /// Once per tick (from `JevBrain::tick`): resolve and ack a **new** decision, if any.
    pub fn poll(&mut self) {
        let Some(d) = self.source.latest() else {
            return;
        };
        if self.last_seen == Some(d.ticket) {
            return;
        }
        self.last_seen = Some(d.ticket);
        let outcome = self.resolve(&d);
        self.source.ack(d.ticket, outcome);
    }

    fn resolve(&mut self, d: &AppliedDecision) -> ApplyOutcome {
        use ApplyOutcome::{Applied, Rejected};
        if self.inert {
            return Rejected(BrainReject::Inert);
        }
        if d.map != self.map {
            return Rejected(BrainReject::WrongMap);
        }
        let Some((_, entry)) = self.ring.iter().find(|(t, _)| *t == d.ticket) else {
            return Rejected(BrainReject::UnknownTicket);
        };
        let pick = |idx: Option<u8>, len: usize| match idx {
            None => Ok(None),
            Some(i) if usize::from(i) < len => Ok(Some(usize::from(i))),
            Some(_) => Err(BrainReject::OffTicketLabel),
        };
        let asked_objective = !entry.objectives.is_empty();
        // jvr: the model's objective answer is followed in the journal, never applied.
        let shadowed = self.shadow || (self.rule && asked_objective);
        let (objective, model_label) = match pick(d.objective, entry.objectives.len()) {
            Ok(i) => (
                i.and_then(|i| entry.objectives[i]),
                i.and_then(|i| entry.labels.get(i).copied()),
            ),
            Err(r) => return Rejected(r),
        };
        let rule_label = entry.rule;
        let posture = match pick(d.posture, entry.postures.len()) {
            Ok(i) => i.map(|i| entry.postures[i]),
            Err(r) => return Rejected(r),
        };
        self.current = Some(Resolved {
            ticket: d.ticket,
            objective,
            posture,
        });
        self.applied_at = self.time;
        // Only an answer to the objective question moves the objective; `carry_on` or a gated-off
        // answer releases to q3's roam (no Released trigger: the model chose to roam).
        if asked_objective && !shadowed {
            self.objective = objective.map(|p| (p, self.time));
        }
        let outcome = if shadowed {
            ApplyOutcome::Shadowed
        } else {
            Applied
        };
        if asked_objective {
            self.open_obj_window(d.ticket, model_label, rule_label);
            return outcome;
        }
        // A newer decision supersedes the one being followed.
        self.close_window(false);
        self.window = Some(Window {
            ticket: d.ticket,
            opened: self.time,
            vitals: self.vitals,
            damage: 0,
            q3_retreat: None,
            biased: false,
        });
        outcome
    }

    /// Open the objective window for an objective decision (closing any older one as superseded).
    fn open_obj_window(
        &mut self,
        ticket: TicketId,
        model_label: Option<&'static str>,
        rule_label: Option<&'static str>,
    ) {
        self.close_obj_window(false);
        let applied = if self.rule { rule_label } else { model_label };
        self.obj_window = Some(ObjWindow {
            ticket,
            opened: self.time,
            vitals: self.vitals,
            damage: 0,
            tier: self.held_tier,
            target: self.objective.map(|(p, _)| p.origin),
            applied,
            rule: rule_label,
            arrived_at: None,
            released: None,
        });
    }

    /// The bot died: drop the current decision and ask again on respawn.
    pub fn on_death(&mut self) {
        // A level change calls `on_level_change` first (which closes the window as a non-death);
        // anything still open here is a real death. (The policy never sees health ≤ 0: `bot_task`
        // calls `on_death` on the death frame before that frame's tick.)
        self.close_window(true);
        self.close_obj_window(true);
        self.current = None;
        self.objective = None;
        self.alive_since = None;
        self.pending_trigger = Some(Trigger::Respawn);
    }

    /// The server changed level: close the open window as a non-death (the `on_death` that
    /// follows then finds nothing to close).
    pub fn on_level_change(&mut self) {
        self.close_window(false);
        self.close_obj_window(false);
    }

    /// A level loaded: new map identity, forget every decision and ticket, ask again.
    pub fn on_map_change(&mut self, map: MapId) {
        self.close_window(false);
        self.close_obj_window(false);
        self.map = map;
        self.current = None;
        self.objective = None;
        self.ring.clear();
        self.pending_trigger = Some(Trigger::MapChange);
    }
}

impl GoalPolicy for JevPolicy {
    fn objective(&mut self, _q: &GoalQuery) -> Option<NavGoal> {
        if self.inert || self.shadow {
            return None;
        }
        self.objective.map(|(p, _)| NavGoal::Position(p.origin))
    }

    fn on_stall(&mut self) {
        self.release_objective("stall");
        self.retreat = None;
    }

    fn retreat_goal(&mut self, q: &GoalQuery) -> Option<NavGoal> {
        if self.inert || !self.pad_retreat {
            return None;
        }
        if self
            .retreat
            .is_none_or(|(t, _)| self.time - t >= RETREAT_REFRESH_S)
        {
            let enemy = q
                .view
                .enemies()
                .filter(|e| !e.is_stale)
                .map(|e| e.origin)
                .min_by(|a, b| (*a - q.pos).length().total_cmp(&(*b - q.pos).length()));
            let pad = match (&self.nav_graph, enemy) {
                (Some(g), Some(enemy)) => pads::retreat_pad(
                    &self.items,
                    &self.memory,
                    g,
                    self.risk(),
                    q.view,
                    self.time,
                    enemy,
                )
                .map(|p| p.origin),
                _ => None,
            };
            self.retreat = Some((self.time, pad));
        }
        self.retreat.and_then(|(_, p)| p).map(NavGoal::Position)
    }

    fn posture(&mut self, q: &PostureQuery) -> Posture {
        if let Some(w) = self.window.as_mut().filter(|w| w.q3_retreat.is_none()) {
            // What q3 alone would have done on the window's first tick (height delta unknown here).
            w.q3_retreat = Some(crate::q3char::wants_to_retreat(q.view, q.ch, None));
        }
        let disengage = !self.inert
            && !self.shadow
            && self.time - self.applied_at < POSTURE_MAX_AGE_S
            && self
                .current
                .as_ref()
                .is_some_and(|c| c.posture == Some(PostureChoice::Disengage))
            // Plan 74 B7: never ADD a retreat at a lava rim — on q2dm3 most lava deaths end in
            // `retreat`. q3's own retreat logic is untouched (this is not more aggression than
            // q3, only less of jev's extra caution where caution kills).
            && !self.at_rim(q.view.self_state().origin);
        if disengage {
            if let Some(w) = &mut self.window {
                w.biased = true;
            }
        }
        Posture { disengage }
    }
}
