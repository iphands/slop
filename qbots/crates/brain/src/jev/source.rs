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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

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
    /// Background refresh at `min_dwell_s`.
    Refresh,
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
            Trigger::Refresh => "refresh",
        }
    }
}

/// The binary posture answer (T12).
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
    /// The posture answer.
    pub posture: Option<PostureChoice>,
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
    /// Not applied, and why.
    Rejected(BrainReject),
}

/// A bot's handle on the decision hub. Implemented by the hub in `crates/qbots` and by the two
/// test doubles below.
pub trait DecisionSource: Send + Sync {
    /// Newest gated decision for this bot, if any (already expired ones read as `None`).
    fn latest(&self) -> Option<AppliedDecision>;
    /// Newest-wins: replaces this bot's pending snapshot. Returns the hub-issued ticket.
    fn submit(&self, state: DecisionState, trig: Trigger) -> TicketId;
    /// The brain's verdict on a decision `latest()` returned.
    fn ack(&self, ticket: TicketId, outcome: ApplyOutcome);
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
    fn submit(&self, _state: DecisionState, _trig: Trigger) -> TicketId {
        // Tickets stay unique even here, so ring logic behaves identically in the control arm.
        TicketId(self.next.fetch_add(1, Ordering::Relaxed) + 1)
    }
    fn ack(&self, _ticket: TicketId, _outcome: ApplyOutcome) {}
}

/// A scripted decider for brain tests: set what `latest()` returns, inspect what was submitted
/// and acked. Tickets count up from 1.
#[derive(Debug, Default)]
pub struct StubDecider {
    next: AtomicU64,
    latest: Mutex<Option<AppliedDecision>>,
    submitted: Mutex<Vec<(TicketId, DecisionState, Trigger)>>,
    acks: Mutex<Vec<(TicketId, ApplyOutcome)>>,
}

impl StubDecider {
    /// Set what `latest()` returns from now on.
    pub fn set_latest(&self, d: Option<AppliedDecision>) {
        *lock(&self.latest) = d;
    }

    /// Every `submit` so far, oldest first.
    pub fn submitted(&self) -> Vec<(TicketId, DecisionState, Trigger)> {
        lock(&self.submitted).clone()
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
    fn submit(&self, state: DecisionState, trig: Trigger) -> TicketId {
        let t = TicketId(self.next.fetch_add(1, Ordering::Relaxed) + 1);
        lock(&self.submitted).push((t, state, trig));
        t
    }
    fn ack(&self, ticket: TicketId, outcome: ApplyOutcome) {
        lock(&self.acks).push((ticket, outcome));
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

    fn state() -> DecisionState {
        DecisionState::from_fields(vec![Field::Map(MapFacts {
            map: MapId::Q2dm1,
            gap: LoadoutGap::None,
        })])
    }

    #[test]
    fn jev_source_null_decider_never_decides_but_issues_unique_tickets() {
        let d = NullDecider::default();
        assert_eq!(d.latest(), None);
        let a = d.submit(state(), Trigger::Respawn);
        let b = d.submit(state(), Trigger::Refresh);
        assert_ne!(a, b);
    }

    #[test]
    fn jev_source_stub_decider_scripts_latest_and_records_traffic() {
        let d = StubDecider::default();
        let t = d.submit(state(), Trigger::Damage);
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
        assert_eq!(d.submitted()[0].2, Trigger::Damage);
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
