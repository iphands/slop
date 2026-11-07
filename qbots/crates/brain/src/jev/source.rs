//! # brain::jev::source — the brain ↔ hub seam (Plan 72 T3)
//!
//! A bot's brain talks to the decision hub only through its own [`DecisionSource`] handle:
//! `submit` a state (newest wins, returns a hub-issued [`TicketId`]), read the `latest` gated
//! decision (a `Copy` value — no allocation on the tick path), and `ack` what it did with it
//! (the only source of the `applied` count). No `async`, so `brain` stays tokio-free and
//! `Brain::tick` stays synchronous.
//!
//! **Tickets** tie an answer to the candidate list it answers: newest-wins replacement means an
//! in-flight answer usually refers to an *older* list than the brain would build now, so the
//! brain resolves an answer only against the ticket it carries. They come from a hub-owned
//! counter because brains are rebuilt per connection session.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use super::question::{QuestionKey, QuestionSpec};
use super::state::{DecisionState, MapId};

/// A hub-issued ask id. Unique per process; never reused across bot reconnects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TicketId(pub u64);

/// Why a bot asked. Events outrank refresh at the hub; `trig=` on every decision line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// Just respawned.
    Respawn,
    /// A loadout gap became critical (≥ 2 s persistent).
    LoadoutGap,
    /// Took damage above threshold (≥ 2 s persistent).
    Damage,
    /// Local danger/heat crossed a threshold (≥ 2 s persistent).
    AreaHeat,
    /// The level changed.
    MapChange,
    /// An enemy came into view (none was visible last tick) — the posture question's main event.
    Contact,
    /// Background refresh at `min_dwell_s`.
    Refresh,
    /// The applied objective was released (arrived, stalled, pad empty, or timed out) — Plan 74.
    Released,
}

impl Trigger {
    /// Events take priority over refresh at the hub's workers.
    pub fn is_event(self) -> bool {
        !matches!(self, Trigger::Refresh)
    }

    /// The `trig=` slug.
    pub fn slug(self) -> &'static str {
        match self {
            Trigger::Respawn => "respawn",
            Trigger::LoadoutGap => "loadout_gap",
            Trigger::Damage => "damage",
            Trigger::AreaHeat => "area_heat",
            Trigger::MapChange => "map_change",
            Trigger::Contact => "contact",
            Trigger::Refresh => "refresh",
            Trigger::Released => "released",
        }
    }
}

/// One ask: the state, the questions about it (whose option order the brain keeps in its
/// ticket ring), and why it is being asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    /// The bot's situation.
    pub state: DecisionState,
    /// The questions, options in offered order.
    pub questions: Vec<QuestionSpec>,
    /// Why now.
    pub trig: Trigger,
}

/// The binary posture answer (T12). The hub stores the option **index**; the brain maps it
/// through its ticket to this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostureChoice {
    /// Add caution: retreat and do not chase.
    Disengage,
    /// Leave it to q3.
    Hold,
}

/// One gated answer, as stored in a bot's slot. Each question field is `None` when that
/// question was not asked or was gated off — a `None` still **supersedes** an older decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppliedDecision {
    /// The ask this answers.
    pub ticket: TicketId,
    /// Hub generation at answer time (a stale generation is discarded before storing).
    pub generation: u64,
    /// The map the ask was made on.
    pub map: MapId,
    /// Index of the chosen objective in **that ticket's** offered list.
    pub objective: Option<u8>,
    /// Index of the chosen posture option in **that ticket's** posture question.
    pub posture: Option<u8>,
}

/// Why the brain did not apply a decision `latest()` returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrainReject {
    /// The ticket has left the brain's ring (it predates the last few asks or a reconnect).
    UnknownTicket,
    /// Asked on a different map.
    WrongMap,
    /// The objective index is outside its ticket's offered list.
    OffTicketLabel,
    /// Past its brain-side lifetime (posture).
    Expired,
    /// The brain is inert (`goal_override` is set).
    Inert,
    /// A newer decision was already applied.
    Superseded,
}

impl BrainReject {
    /// The `why=` slug on `EVT jev_reject`.
    pub fn slug(self) -> &'static str {
        match self {
            BrainReject::UnknownTicket => "unknown_ticket",
            BrainReject::WrongMap => "wrong_map",
            BrainReject::OffTicketLabel => "off_ticket_label",
            BrainReject::Expired => "expired",
            BrainReject::Inert => "inert",
            BrainReject::Superseded => "superseded",
        }
    }
}

/// What the brain did with a decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// Applied — counted as `applied`.
    Applied,
    /// Resolved by a **shadow** brain (Plan 73): followed and journalled, never steered.
    Shadowed,
    /// Not applied, and why.
    Rejected(BrainReject),
}

/// What happened after a decision (Plan 73 T5), measured by the brain over a window that opens
/// when the decision is resolved and closes after [`OUTCOME_WINDOW_S`] of brain time, on death,
/// on a level change, or when a newer decision supersedes it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Outcome {
    /// Brain seconds actually observed.
    pub secs: f32,
    /// The bot died inside the window.
    pub died: bool,
    /// Health at close − health at open.
    pub hp_delta: i32,
    /// Armor at close − armor at open.
    pub armor_delta: i32,
    /// Frags at close − frags at open.
    pub frags_delta: i32,
    /// Health lost to hits inside the window (sum of drops).
    pub damage_taken: u32,
    /// q3's own `wants_to_retreat` on the first tick of the window — what q3 would have done.
    pub q3_retreat: Option<bool>,
    /// The decision actually changed q3's inputs (a `disengage` was in force, not in shadow).
    pub biased: bool,
    /// Set for an objective decision (Plan 74 T7) — its window is [`OBJECTIVE_WINDOW_S`] long.
    pub objective: Option<ObjectiveOutcome>,
}

/// What followed an objective decision (Plan 74 T7).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ObjectiveOutcome {
    /// The label the bot acted on: the model's for `jev`, the rule's for `jev_rule`; `None` when
    /// nothing was applied (a gated-off answer).
    pub applied: Option<&'static str>,
    /// The code rule's choice from the same candidates — applied only by `jev_rule`, journalled
    /// for both arms so model/rule agreement is measurable.
    pub rule: Option<&'static str>,
    /// Reached the applied pad inside the window.
    pub arrived: bool,
    /// Brain seconds from the window opening to arrival.
    pub secs_to_arrive: Option<f32>,
    /// The held weapon's power tier was higher at close than at open.
    pub upgraded: bool,
}

/// How long an outcome window stays open (brain time, seconds).
pub const OUTCOME_WINDOW_S: f32 = 10.0;
/// How long an objective decision's outcome window stays open (brain time, seconds) — the
/// objective's own release cap, so a walk to a far pad can finish inside it.
pub const OBJECTIVE_WINDOW_S: f32 = 30.0;

/// A bot's handle on the decision hub. Implemented by the hub in `crates/qbots` and by the two
/// test doubles below.
pub trait DecisionSource: Send + Sync {
    /// Newest gated decision for this bot, if any (already expired ones read as `None`).
    fn latest(&self) -> Option<AppliedDecision>;
    /// Newest-wins: replaces this bot's pending ask. `Some(ticket)` if accepted; `None` if the
    /// hub's per-bot pacing refused it (an event inside `event_floor_s`, or a refresh before
    /// it is due) — a refused ask must not take a slot in the brain's ticket ring.
    fn submit(&self, ask: Ask) -> Option<TicketId>;
    /// Is a background refresh of question `key` due for this bot? Cheap — lets the brain skip
    /// building a state on the ~all ticks where a refresh would be refused. Each question key
    /// keeps its own pacing (Plan 74 T4), so a roaming objective ask never paces out a contact
    /// posture ask.
    fn refresh_due(&self, key: QuestionKey) -> bool;
    /// The brain's verdict on a decision `latest()` returned.
    fn ack(&self, ticket: TicketId, outcome: ApplyOutcome);
    /// What happened after an applied decision (Plan 73 journal). Best effort, never blocks.
    fn outcome(&self, ticket: TicketId, outcome: Outcome);
}

/// Never decides. `JevStatic` (the primary A/B control) and offline tests use it.
#[derive(Debug, Default)]
pub struct NullDecider {
    next: AtomicU64,
}

impl DecisionSource for NullDecider {
    fn latest(&self) -> Option<AppliedDecision> {
        None
    }
    fn submit(&self, _ask: Ask) -> Option<TicketId> {
        // Tickets stay unique even here, so ring logic behaves identically in the control arm.
        Some(TicketId(self.next.fetch_add(1, Ordering::Relaxed) + 1))
    }
    fn refresh_due(&self, _key: QuestionKey) -> bool {
        false
    }
    fn ack(&self, _ticket: TicketId, _outcome: ApplyOutcome) {}
    fn outcome(&self, _ticket: TicketId, _outcome: Outcome) {}
}

/// A scripted decider for brain tests: set what `latest()` and `refresh_due()` return and
/// whether asks are refused; inspect what was submitted and acked. Tickets count up from 1.
#[derive(Debug, Default)]
pub struct StubDecider {
    next: AtomicU64,
    latest: Mutex<Option<AppliedDecision>>,
    submitted: Mutex<Vec<(TicketId, Ask)>>,
    acks: Mutex<Vec<(TicketId, ApplyOutcome)>>,
    outcomes: Mutex<Vec<(TicketId, Outcome)>>,
    refuse: AtomicBool,
    refresh_due: AtomicBool,
}

impl StubDecider {
    /// Set what `latest()` returns from now on.
    pub fn set_latest(&self, d: Option<AppliedDecision>) {
        *lock(&self.latest) = d;
    }

    /// Every accepted `submit` so far, oldest first.
    pub fn submitted(&self) -> Vec<(TicketId, Ask)> {
        lock(&self.submitted).clone()
    }

    /// Every `outcome` so far, oldest first.
    pub fn outcomes(&self) -> Vec<(TicketId, Outcome)> {
        lock(&self.outcomes).clone()
    }

    /// Make `submit` refuse (as the hub's pacing would) from now on.
    pub fn set_refuse(&self, refuse: bool) {
        self.refuse.store(refuse, Ordering::Relaxed);
    }

    /// Set what `refresh_due()` returns.
    pub fn set_refresh_due(&self, due: bool) {
        self.refresh_due.store(due, Ordering::Relaxed);
    }

    /// Every `ack` so far, oldest first.
    pub fn acks(&self) -> Vec<(TicketId, ApplyOutcome)> {
        lock(&self.acks).clone()
    }
}

impl DecisionSource for StubDecider {
    fn latest(&self) -> Option<AppliedDecision> {
        *lock(&self.latest)
    }
    fn submit(&self, ask: Ask) -> Option<TicketId> {
        if self.refuse.load(Ordering::Relaxed) {
            return None;
        }
        let t = TicketId(self.next.fetch_add(1, Ordering::Relaxed) + 1);
        lock(&self.submitted).push((t, ask));
        Some(t)
    }
    fn refresh_due(&self, _key: QuestionKey) -> bool {
        self.refresh_due.load(Ordering::Relaxed)
    }
    fn ack(&self, ticket: TicketId, outcome: ApplyOutcome) {
        lock(&self.acks).push((ticket, outcome));
    }
    fn outcome(&self, ticket: TicketId, outcome: Outcome) {
        lock(&self.outcomes).push((ticket, outcome));
    }
}

/// Lock, recovering from poisoning: a test double must not cascade one panicking test into
/// unrelated failures.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jev::state::{DecisionState, Field, LoadoutGap, MapFacts};

    fn ask(trig: Trigger) -> Ask {
        Ask {
            state: DecisionState::from_fields(vec![Field::Map(MapFacts {
                map: MapId::Q2dm1,
                gap: LoadoutGap::None,
            })]),
            questions: Vec::new(),
            trig,
        }
    }

    #[test]
    fn jev_source_null_decider_never_decides_but_issues_unique_tickets() {
        let d = NullDecider::default();
        assert_eq!(d.latest(), None);
        let a = d.submit(ask(Trigger::Respawn));
        let b = d.submit(ask(Trigger::Refresh));
        assert!(a.is_some() && b.is_some());
        assert_ne!(a, b);
        assert!(!d.refresh_due(QuestionKey::Posture));
    }

    #[test]
    fn jev_source_stub_decider_scripts_latest_and_records_traffic() {
        let d = StubDecider::default();
        let t = d.submit(ask(Trigger::Damage)).expect("accepted");
        assert_eq!(t, TicketId(1));
        let dec = AppliedDecision {
            ticket: t,
            generation: 0,
            map: MapId::Q2dm1,
            objective: Some(1),
            posture: None,
        };
        d.set_latest(Some(dec));
        assert_eq!(d.latest(), Some(dec));
        d.ack(t, ApplyOutcome::Rejected(BrainReject::OffTicketLabel));
        assert_eq!(
            d.acks(),
            vec![(t, ApplyOutcome::Rejected(BrainReject::OffTicketLabel))]
        );
        assert_eq!(d.submitted()[0].1.trig, Trigger::Damage);
        d.set_refuse(true);
        assert_eq!(
            d.submit(ask(Trigger::Respawn)),
            None,
            "a refused ask gets no ticket"
        );
        assert_eq!(d.submitted().len(), 1);
        d.set_refresh_due(true);
        assert!(d.refresh_due(QuestionKey::Posture));
    }

    #[test]
    fn jev_source_trait_object_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn DecisionSource>();
    }

    #[test]
    fn jev_source_refresh_is_the_only_non_event() {
        assert!(!Trigger::Refresh.is_event());
        assert!(Trigger::Respawn.is_event() && Trigger::MapChange.is_event());
    }
}
