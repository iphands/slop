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
//! While inert (`goal_override` set — the `spawn-to-*` harnesses) nothing is asked, a new
//! decision is acked `Inert`, and the posture stays q3's own.

use std::collections::VecDeque;
use std::sync::Arc;

use world::NavGraph;

use crate::brains::core::MapItem;
use crate::brains::q3::{GoalPolicy, GoalQuery, Posture, PostureQuery};
use crate::jev::question::posture_question;
use crate::jev::source::{
    AppliedDecision, ApplyOutcome, Ask, BrainReject, DecisionSource, Outcome, PostureChoice,
    TicketId, Trigger, OUTCOME_WINDOW_S,
};
use crate::jev::state::{DecisionState, MapContext, MapId, SelfSummary};
use crate::nav::NavGoal;
use crate::perception::Worldview;

/// How long an applied `disengage` biases q3 (brain time, seconds) — about one fight.
pub const POSTURE_MAX_AGE_S: f32 = 6.0;
/// The window `recent` damage is summed over (brain time, seconds).
const DAMAGE_WINDOW_S: f32 = 10.0;

/// How many recent asks the brain can still resolve an answer against.
pub const RING: usize = 4;

/// What one ask offered, in offered order — the only list its answer may resolve against.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TicketEntry {
    /// The objective options' goals (index = the answer's `objective`).
    pub objectives: Vec<NavGoal>,
    /// The posture options (index = the answer's `posture`).
    pub postures: Vec<PostureChoice>,
}

/// An accepted decision, resolved through its ticket.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    /// The ask it answers.
    pub ticket: TicketId,
    /// The chosen objective's goal, if that question was answered decisively.
    pub objective: Option<NavGoal>,
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
            shadow: false,
        }
    }

    /// A shadow policy: asks and journals like jev, never biases q3 (the arena's control).
    pub fn new_shadow(source: Arc<dyn DecisionSource>) -> Self {
        Self {
            shadow: true,
            ..Self::new(source)
        }
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
            },
        );
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
        let enemy = view.enemies().any(|e| !e.is_stale);
        let contact = enemy && !self.saw_enemy;
        self.saw_enemy = enemy;
        if self.inert || !enemy {
            return;
        }
        let trig = if contact {
            Trigger::Contact
        } else if hit > 0 {
            Trigger::Damage
        } else if self.source.refresh_due() {
            Trigger::Refresh
        } else {
            return;
        };
        self.ask(view, trig);
    }

    /// Build the closed state, ask the posture question, and remember what it offered.
    fn ask(&mut self, view: &Worldview, trig: Trigger) {
        let me = SelfSummary {
            recent_damage: self.damage.iter().map(|(_, n)| n).sum(),
            secs_since_damage: self.damage.back().map(|(t, _)| self.time - t),
        };
        let ctx = MapContext {
            map: self.map,
            nav_graph: self.nav_graph.as_deref(),
        };
        let ask = Ask {
            state: DecisionState::build(view, &ctx, &me),
            questions: vec![posture_question()],
            trig,
        };
        if let Some(ticket) = self.source.submit(ask) {
            // Same order as `posture_question()`'s options.
            let postures = vec![PostureChoice::Disengage, PostureChoice::Hold];
            self.record_ticket(
                ticket,
                TicketEntry {
                    objectives: Vec::new(),
                    postures,
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
        let objective = match pick(d.objective, entry.objectives.len()) {
            Ok(i) => i.map(|i| entry.objectives[i].clone()),
            Err(r) => return Rejected(r),
        };
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
        let outcome = if self.shadow {
            ApplyOutcome::Shadowed
        } else {
            Applied
        };
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

    /// The bot died: drop the current decision and ask again on respawn.
    pub fn on_death(&mut self) {
        // `bot_task` also calls `on_death` on a level change; only a real death has health ≤ 0.
        let died = self.vitals.0 <= 0;
        self.close_window(died);
        self.current = None;
        self.pending_trigger = Some(Trigger::Respawn);
    }

    /// A level loaded: new map identity, forget every decision and ticket, ask again.
    pub fn on_map_change(&mut self, map: MapId) {
        self.close_window(false);
        self.map = map;
        self.current = None;
        self.ring.clear();
        self.pending_trigger = Some(Trigger::MapChange);
    }
}

impl GoalPolicy for JevPolicy {
    fn objective(&mut self, _q: &GoalQuery) -> Option<NavGoal> {
        // T10 makes the current decision's objective steer (held until released).
        None
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
                .is_some_and(|c| c.posture == Some(PostureChoice::Disengage));
        if disengage {
            if let Some(w) = &mut self.window {
                w.biased = true;
            }
        }
        Posture { disengage }
    }
}
