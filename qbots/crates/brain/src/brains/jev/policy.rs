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
//! What a current decision *does* — hold an objective until released (T10), bias the posture
//! (T12) — lands with those tasks; until then `objective()`/`posture()` have no opinion. No ask
//! is ever sent before T10 either (a request with no questions must not reach the endpoint), so
//! in production nothing reaches this path yet. While inert (`goal_override` set — the
//! `spawn-to-*` harnesses) a new decision is acked `Inert` and never becomes current.

use std::collections::VecDeque;
use std::sync::Arc;

use world::NavGraph;

use crate::brains::core::MapItem;
use crate::brains::q3::{GoalPolicy, GoalQuery, Posture, PostureQuery};
use crate::jev::source::{
    AppliedDecision, ApplyOutcome, BrainReject, DecisionSource, PostureChoice, TicketId, Trigger,
};
use crate::jev::state::MapId;
use crate::nav::NavGoal;

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
        Applied
    }

    /// The bot died: drop the current decision and ask again on respawn.
    pub fn on_death(&mut self) {
        self.current = None;
        self.pending_trigger = Some(Trigger::Respawn);
    }

    /// A level loaded: new map identity, forget every decision and ticket, ask again.
    pub fn on_map_change(&mut self, map: MapId) {
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

    fn posture(&mut self, _q: &PostureQuery) -> Posture {
        // T12 makes a current `disengage` bias the posture. Never biased while inert.
        Posture::default()
    }
}
