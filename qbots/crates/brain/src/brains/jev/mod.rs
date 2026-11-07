//! # brain::brains::jev — the jev brain (Plan 72): q3, with high-level choices from a model
//!
//! `JevBrain` **composes** a real q3 brain — `Q3Brain<JevPolicy>` — and copies none of it. q3
//! asks its [`JevPolicy`] for the roam-fallback goal and a posture bias; everything else (FSM,
//! aim, weapon choice, dodge, recovery) is q3's own code path, so a q3 fix is a jev fix.
//!
//! The wrapper exists for what q3 cannot see:
//! - `set_map`: q3 discards `BrainMap::items` by design, but candidate goals need them — the
//!   wrapper copies the map facts into the policy, then hands the map to q3.
//! - `tick`: the policy is **inert** while `goal_override` is set (the `spawn-to-*` harnesses).
//!   q3 consults the posture *before* its own `goal_override` branch, so this flag is what makes
//!   jev inert there — q3's structure alone would not.
//! - `status()` delegates **verbatim**, so `"retreat"` etc. read exactly as for q3.
//!
//! `JevStatic` (the primary A/B control) is this same brain with a `NullDecider`.

mod policy;

pub use policy::{JevPolicy, Resolved, TicketEntry, RING};

use std::sync::Arc;

use crate::brains::core::{Brain, BrainContext, BrainMap, BrainOutput};
use crate::brains::q3::Q3Brain;
use crate::jev::source::DecisionSource;
use crate::jev::state::MapId;
use crate::q3char::Q3Character;

/// The jev brain. See the module doc.
pub struct JevBrain {
    inner: Q3Brain<JevPolicy>,
}

impl JevBrain {
    /// A jev brain with character `ch` that reads decisions from `source`.
    pub fn new(ch: Q3Character, source: Arc<dyn DecisionSource>) -> Self {
        Self {
            inner: Q3Brain::new_with_policy(ch, JevPolicy::new(source)),
        }
    }

    /// The policy (tests, telemetry).
    pub fn policy(&self) -> &JevPolicy {
        self.inner.policy()
    }

    /// Mutable policy access (the ask path records tickets through it; tests seed it).
    pub fn policy_mut(&mut self) -> &mut JevPolicy {
        self.inner.policy_mut()
    }
}

impl Brain for JevBrain {
    fn set_map(&mut self, map: BrainMap) {
        self.inner.policy_mut().set_map_facts(
            map.roam_nodes.clone(),
            map.nav_graph.clone(),
            map.items.clone(),
        );
        self.inner.set_map(map);
    }

    fn tick(&mut self, ctx: BrainContext) -> BrainOutput {
        let policy = self.inner.policy_mut();
        policy.set_inert(ctx.goal_override.is_some());
        // One lock + one Copy: resolve and ack a new decision, if one arrived.
        policy.poll();
        self.inner.tick(ctx)
    }

    fn on_kill(&mut self) {
        self.inner.on_kill();
    }

    fn on_death(&mut self) {
        self.inner.on_death();
        self.inner.policy_mut().on_death();
    }

    fn on_map_change(&mut self, map: &str) {
        // The wire's map name is matched against a closed table and not kept.
        self.inner.policy_mut().on_map_change(MapId::from_name(map));
    }

    fn heatmap_weights(&self) -> (f32, f32) {
        self.inner.heatmap_weights()
    }

    fn status(&self) -> &str {
        self.inner.status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brains::core::MapItem;
    use crate::jev::source::{
        AppliedDecision, ApplyOutcome, BrainReject, StubDecider, TicketId, Trigger,
    };
    use crate::jev::state::MapId;
    use crate::nav::NavGoal;
    use crate::nav_mode::StubNav;
    use crate::perception::{EntityClass, Worldview};
    use client::parse::ConfigStrings;
    use glam::Vec3;
    use q2proto::Frame;
    use world::NavGraph;

    fn empty_view() -> Worldview {
        Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0)
    }

    fn map() -> BrainMap {
        BrainMap {
            roam_nodes: vec![0, 1],
            nav_graph: Arc::new(NavGraph::from_raw(
                vec![[0.0, 0.0, 0.0], [64.0, 0.0, 0.0]],
                vec![Vec::new(); 2],
            )),
            roam_as_position: false,
            items: vec![MapItem {
                class: EntityClass::ItemArmor,
                origin: Vec3::new(512.0, 0.0, 0.0),
                nav_node: Some(1),
            }],
        }
    }

    #[test]
    fn jev_brain_set_map_keeps_items_q3_discards() {
        let mut b = JevBrain::new(Q3Character::default(), Arc::new(StubDecider::default()));
        b.set_map(map());
        assert_eq!(b.policy().items().len(), 1, "the policy kept the BSP items");
        assert_eq!(b.policy().roam_nodes(), &[0, 1]);
        assert!(b.policy().nav_graph().is_some());
    }

    #[test]
    fn jev_brain_goal_override_makes_the_policy_inert_and_silent() {
        // A guard test: T7's policy never asks or biases yet, so this proves the inert flag
        // tracks goal_override and that nothing is submitted — nothing more.
        let stub = Arc::new(StubDecider::default());
        stub.set_refresh_due(true);
        stub.set_latest(Some(AppliedDecision {
            ticket: TicketId(1),
            generation: 0,
            map: MapId::Q2dm1,
            objective: Some(0),
            posture: Some(0),
        }));
        let mut b = JevBrain::new(Q3Character::default(), stub.clone());
        b.set_map(map());
        let view = empty_view();
        let mut nav = StubNav {
            pursue: Some(Vec3::new(200.0, 0.0, 0.0)),
            ..Default::default()
        };
        let goal = NavGoal::Position(Vec3::new(1000.0, 0.0, 0.0));
        b.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: Some(goal.clone()),
        });
        assert!(b.policy().is_inert());
        assert_eq!(nav.last_goal, Some(goal), "the override goal is followed");
        assert!(stub.submitted().is_empty(), "an inert policy never asks");
        b.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 2,
            goal_override: None,
        });
        assert!(!b.policy().is_inert(), "inertness is per tick");
    }

    fn tick_once(b: &mut JevBrain, override_goal: Option<NavGoal>) {
        let view = empty_view();
        let mut nav = StubNav::default();
        b.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: override_goal,
        });
    }

    fn decision(ticket: u64, map: MapId, objective: Option<u8>) -> AppliedDecision {
        AppliedDecision {
            ticket: TicketId(ticket),
            generation: 0,
            map,
            objective,
            posture: None,
        }
    }

    /// A brain on q2dm1 with ticket 7 offering two objective goals.
    fn seeded() -> (JevBrain, Arc<StubDecider>) {
        let stub = Arc::new(StubDecider::default());
        let mut b = JevBrain::new(Q3Character::default(), stub.clone());
        b.on_map_change("q2dm1");
        b.policy_mut().record_ticket(
            TicketId(7),
            TicketEntry {
                objectives: vec![
                    NavGoal::Waypoint(3),
                    NavGoal::Position(Vec3::new(1.0, 2.0, 3.0)),
                ],
                postures: Vec::new(),
            },
        );
        (b, stub)
    }

    #[test]
    fn jev_apply_resolves_against_its_own_ticket_and_acks_once() {
        let (mut b, stub) = seeded();
        stub.set_latest(Some(decision(7, MapId::Q2dm1, Some(1))));
        tick_once(&mut b, None);
        tick_once(&mut b, None);
        assert_eq!(
            stub.acks(),
            vec![(TicketId(7), ApplyOutcome::Applied)],
            "acked once"
        );
        assert_eq!(
            b.policy().current().and_then(|r| r.objective.clone()),
            Some(NavGoal::Position(Vec3::new(1.0, 2.0, 3.0)))
        );
    }

    #[test]
    fn jev_apply_rejects_wrong_map_off_ticket_unknown_and_inert() {
        let cases = [
            (
                decision(7, MapId::Q2dm3, Some(0)),
                None,
                BrainReject::WrongMap,
            ),
            (
                decision(7, MapId::Q2dm1, Some(2)),
                None,
                BrainReject::OffTicketLabel,
            ),
            (
                decision(9, MapId::Q2dm1, Some(0)),
                None,
                BrainReject::UnknownTicket,
            ),
            (
                decision(7, MapId::Q2dm1, Some(0)),
                Some(NavGoal::Waypoint(1)),
                BrainReject::Inert,
            ),
        ];
        for (d, override_goal, why) in cases {
            let (mut b, stub) = seeded();
            stub.set_latest(Some(d));
            tick_once(&mut b, override_goal);
            assert_eq!(
                stub.acks(),
                vec![(d.ticket, ApplyOutcome::Rejected(why))],
                "{why:?}"
            );
            assert!(b.policy().current().is_none(), "{why:?}: nothing applied");
        }
    }

    #[test]
    fn jev_apply_ring_forgets_the_oldest_ticket() {
        let (mut b, stub) = seeded();
        for t in 100..100 + RING as u64 {
            b.policy_mut()
                .record_ticket(TicketId(t), TicketEntry::default());
        }
        stub.set_latest(Some(decision(7, MapId::Q2dm1, None)));
        tick_once(&mut b, None);
        assert_eq!(
            stub.acks(),
            vec![(
                TicketId(7),
                ApplyOutcome::Rejected(BrainReject::UnknownTicket)
            )]
        );
    }

    #[test]
    fn jev_apply_death_and_level_change_clear_and_mark_a_trigger() {
        let (mut b, stub) = seeded();
        stub.set_latest(Some(decision(7, MapId::Q2dm1, Some(0))));
        tick_once(&mut b, None);
        assert!(b.policy().current().is_some());
        b.on_death();
        assert!(b.policy().current().is_none());
        assert_eq!(b.policy().pending_trigger(), Some(Trigger::Respawn));
        b.on_map_change("q2dm2");
        assert_eq!(b.policy().map(), MapId::Q2dm2);
        assert_eq!(b.policy().pending_trigger(), Some(Trigger::MapChange));
        // The ring was cleared with the level: the old ticket no longer resolves.
        stub.set_latest(Some(decision(8, MapId::Q2dm2, None)));
        tick_once(&mut b, None);
        assert_eq!(
            stub.acks().last(),
            Some(&(
                TicketId(8),
                ApplyOutcome::Rejected(BrainReject::UnknownTicket)
            ))
        );
    }

    #[test]
    fn jev_brain_status_delegates_verbatim() {
        let b = JevBrain::new(Q3Character::default(), Arc::new(StubDecider::default()));
        assert_eq!(b.status(), "seek-ltg", "no jev: prefix");
    }
}
