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

pub mod pads;
mod policy;

pub use policy::{
    JevPolicy, Resolved, TicketEntry, ARRIVE_RADIUS, OBJECTIVE_MAX_S, RESPAWN_SETTLE_S, RING,
};

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

    /// A **shadow** jev brain (Plan 73): q3 exactly as `jev_static`, asking and journalling the
    /// posture question through `source` without ever acting on it.
    /// The `jvr` control (Plan 74): the pickup objective comes from [`pads::PadCandidates::rule_pick`],
    /// applied when the hub accepts the ask; the model's objective answer is journalled, never
    /// applied. Posture plays exactly as `jev`.
    pub fn new_rule(ch: Q3Character, source: Arc<dyn DecisionSource>) -> Self {
        Self {
            inner: Q3Brain::new_with_policy(ch, JevPolicy::new_rule(source)),
        }
    }

    pub fn new_shadow(ch: Q3Character, source: Arc<dyn DecisionSource>) -> Self {
        Self {
            inner: Q3Brain::new_with_policy(ch, JevPolicy::new_shadow(source)),
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
        if let Some(cm) = ctx.cm {
            policy.ensure_rim(cm);
        }
        // Ask (posture, in a fight) — one short lock + notify; never waits on the model.
        policy.pre_tick(ctx.view, ctx.dt);
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

    fn on_level_change(&mut self) {
        self.inner.policy_mut().on_level_change();
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
                weapon: None,
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

    /// A pad for item `item` at `x` on the x axis.
    fn pad_at(item: usize, x: f32) -> super::pads::Pad {
        super::pads::Pad {
            item,
            origin: Vec3::new(x, 0.0, 0.0),
            cost: x,
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
                objectives: vec![Some(pad_at(3, 768.0)), Some(pad_at(0, 1.0))],
                ..TicketEntry::default()
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
            b.policy().current().and_then(|r| r.objective),
            Some(pad_at(0, 1.0))
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

    /// Our bot holds a railgun at full health (sarge ⇒ q3 wants to CHASE), one enemy at 200 u.
    fn fight_view() -> Worldview {
        fight_view_hp(100)
    }

    fn fight_view_hp(hp: i16) -> Worldview {
        use q2proto::EntityState;
        let mut frame = Frame::default();
        frame.playerstate.gunindex = 1;
        frame.playerstate.stats[1] = hp;
        frame.playerstate.stats[3] = 8;
        frame.playerstate.stats[5] = 100;
        frame.entities = vec![EntityState {
            number: 9,
            origin: [200.0, 0.0, 0.0],
            angles: [0.0, 180.0, 0.0],
            modelindex: 255,
            ..Default::default()
        }];
        let mut cs = ConfigStrings::default();
        cs.set(32 + 1, "models/weapons/v_rail/tris.md2");
        Worldview::from_frame(&frame, &cs, 0)
    }

    fn tick_view(b: &mut JevBrain, view: &Worldview) {
        let mut nav = StubNav {
            pursue: Some(Vec3::new(200.0, 0.0, 0.0)),
            ..Default::default()
        };
        b.tick(BrainContext {
            view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: None,
        });
    }

    fn posture_decision(ticket: u64, choice: u8) -> AppliedDecision {
        AppliedDecision {
            ticket: TicketId(ticket),
            generation: 0,
            map: MapId::Q2dm1,
            objective: None,
            posture: Some(choice),
        }
    }

    fn fight_brain() -> (JevBrain, Arc<StubDecider>) {
        let stub = Arc::new(StubDecider::default());
        let mut b = JevBrain::new(Q3Character::sarge(), stub.clone());
        b.on_map_change("q2dm1");
        (b, stub)
    }

    #[test]
    fn jev_posture_contact_asks_the_production_posture_question_once() {
        let (mut b, stub) = fight_brain();
        stub.set_refresh_due(true);
        tick_once(&mut b, None); // no enemy: nothing to ask about, even with a refresh due
        assert!(stub.submitted().is_empty());
        stub.set_refresh_due(false);
        tick_view(&mut b, &fight_view());
        tick_view(&mut b, &fight_view()); // still in contact, not hit, no refresh due
        let sent = stub.submitted();
        assert_eq!(sent.len(), 1, "one ask per contact");
        assert_eq!(sent[0].1.trig, Trigger::Contact);
        assert_eq!(
            sent[0].1.questions,
            vec![crate::jev::question::posture_question()]
        );
        assert_eq!(
            sent[0].1.state.get(crate::jev::state::FieldKey::Map),
            Some(&crate::jev::state::Field::Map(
                crate::jev::state::MapFacts {
                    map: MapId::Q2dm1,
                    gap: crate::jev::state::LoadoutGap::None,
                }
            ))
        );
    }

    #[test]
    fn jev_posture_disengage_turns_a_chase_state_into_retreat_then_expires() {
        let (mut b, stub) = fight_brain();
        let view = fight_view();
        tick_view(&mut b, &view); // contact → ask (ticket 1); q3 alone fights (sarge wants chase)
        assert_eq!(b.status(), "fight");
        stub.set_latest(Some(posture_decision(1, 0))); // 0 = disengage
        tick_view(&mut b, &view);
        assert_eq!(
            b.status(),
            "retreat",
            "disengage forces retreat AND clears chase"
        );
        assert_eq!(stub.acks(), vec![(TicketId(1), ApplyOutcome::Applied)]);
        for _ in 0..65 {
            tick_view(&mut b, &view); // 6.5 s of brain time
        }
        assert_ne!(
            b.status(),
            "retreat",
            "the bias lapses after POSTURE_MAX_AGE_S"
        );
    }

    /// A corridor −512..+512 (x, 256 u apart): a health pad at −512 (behind us, away from the
    /// enemy at +200) and one at +512 (past the enemy).
    fn retreat_map(away_pad: bool) -> BrainMap {
        let xs: Vec<f32> = (-2..=2).map(|i| i as f32 * 256.0).collect();
        let nodes = xs.iter().map(|x| [*x, 0.0, 0.0]).collect();
        let adj = (0..5usize)
            .map(|i| {
                [i.checked_sub(1), (i < 4).then_some(i + 1)]
                    .into_iter()
                    .flatten()
                    .map(|j| (j, 256.0))
                    .collect()
            })
            .collect();
        let health = |node: usize| MapItem {
            class: EntityClass::ItemHealth,
            weapon: None,
            origin: Vec3::new(xs[node], 0.0, 0.0),
            nav_node: Some(node),
        };
        let mut items = vec![health(4)];
        if away_pad {
            items.push(health(0));
        }
        BrainMap {
            roam_nodes: vec![2],
            nav_graph: Arc::new(NavGraph::from_raw(nodes, adj)),
            roam_as_position: false,
            items,
        }
    }

    /// Disengage a hurt sarge (enemy at +200) on `map`; the goal the navigator gets.
    fn retreat_goal_on(map: BrainMap) -> Option<NavGoal> {
        let (mut b, stub) = fight_brain();
        b.set_map(map);
        b.on_map_change("q2dm1");
        b.policy_mut().set_pad_retreat(true); // off by default since B8
        let view = fight_view_hp(60);
        tick_view(&mut b, &view);
        stub.set_latest(Some(posture_decision(1, 0)));
        let mut nav = StubNav::default();
        b.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 2,
            goal_override: None,
        });
        assert_eq!(b.status(), "retreat");
        nav.last_goal
    }

    #[test]
    fn jev_retreat_heads_for_a_pad_away_from_the_enemy_never_past_it() {
        assert_eq!(
            retreat_goal_on(retreat_map(true)),
            Some(NavGoal::Position(Vec3::new(-512.0, 0.0, 0.0))),
            "the health pad behind us"
        );
        // Only a pad beyond the enemy: q3's own retreat (step 256 u straight back).
        assert_eq!(
            retreat_goal_on(retreat_map(false)),
            Some(NavGoal::Position(Vec3::new(-256.0, 0.0, 0.0))),
        );
    }

    #[test]
    fn jev_posture_disengage_is_withheld_at_a_lava_rim() {
        // The bot stands on node 0 (the origin). Rim there: the disengage is not applied and
        // sarge keeps fighting; no rim: the same answer forces the retreat.
        for (rim, want) in [(vec![256.0, 0.0], "fight"), (vec![0.0, 0.0], "retreat")] {
            let (mut b, stub) = fight_brain();
            b.set_map(map());
            b.on_map_change("q2dm1");
            b.policy_mut().set_rim(rim);
            let view = fight_view();
            tick_view(&mut b, &view);
            stub.set_latest(Some(posture_decision(1, 0)));
            tick_view(&mut b, &view);
            assert_eq!(b.status(), want);
        }
    }

    #[test]
    fn jev_posture_pinned_disengage_never_freezes_the_bot() {
        // Plan 72 W4 / T12: the enemy leaves the worldview while a disengage is in force — q3's
        // own exits (enemy_alive false) must still return the bot to roaming, with movement.
        let (mut b, stub) = fight_brain();
        tick_view(&mut b, &fight_view());
        stub.set_latest(Some(posture_decision(1, 0)));
        tick_view(&mut b, &fight_view());
        assert_eq!(b.status(), "retreat");
        let empty = empty_view();
        let mut nav = StubNav {
            pursue: Some(Vec3::new(200.0, 0.0, 0.0)),
            ..Default::default()
        };
        let out = b.tick(BrainContext {
            view: &empty,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 2,
            goal_override: None,
        });
        assert_eq!(
            b.status(),
            "seek-ltg",
            "the pinned posture does not trap the FSM"
        );
        assert!(
            out.intent.forward != 0.0 || out.intent.side != 0.0,
            "and the bot moves"
        );
    }

    #[test]
    fn jev_posture_hold_is_a_no_op() {
        let (mut b, stub) = fight_brain();
        let view = fight_view();
        tick_view(&mut b, &view);
        stub.set_latest(Some(posture_decision(1, 1))); // 1 = hold
        tick_view(&mut b, &view);
        assert_eq!(b.status(), "fight", "hold leaves q3's own choice");
        assert_eq!(stub.acks(), vec![(TicketId(1), ApplyOutcome::Applied)]);
    }

    #[test]
    fn jev_posture_inert_never_asks_or_biases() {
        let (mut b, stub) = fight_brain();
        let view = fight_view();
        let mut nav = StubNav::default();
        b.tick(BrainContext {
            view: &view,
            nav: Some(&mut nav),
            cm: None,
            dt: 0.1,
            ticks: 1,
            goal_override: Some(NavGoal::Waypoint(1)),
        });
        assert!(
            stub.submitted().is_empty(),
            "the harnesses never ask the model"
        );
    }

    #[test]
    fn jev_outcome_window_reports_what_happened_after_a_disengage() {
        let (mut b, stub) = fight_brain();
        tick_view(&mut b, &fight_view()); // contact → ticket 1
        stub.set_latest(Some(posture_decision(1, 0))); // disengage
        tick_view(&mut b, &fight_view()); // resolved: window opens at 100 hp
        tick_view(&mut b, &fight_view_hp(80)); // a 20-point hit inside the window
        for _ in 0..100 {
            tick_view(&mut b, &fight_view_hp(80));
        }
        let o = stub.outcomes();
        assert_eq!(o.len(), 1, "one window, closed at OUTCOME_WINDOW_S");
        let (t, out) = o[0];
        assert_eq!(t, TicketId(1));
        assert!(!out.died);
        assert_eq!((out.hp_delta, out.damage_taken), (-20, 20));
        assert!(out.biased, "the disengage was in force");
        assert_eq!(
            out.q3_retreat,
            Some(false),
            "q3 alone (sarge, railgun) would not retreat"
        );
        assert!((out.secs - 10.0).abs() < 0.11);
    }

    #[test]
    fn jev_outcome_death_and_level_change_close_the_window() {
        let (mut b, stub) = fight_brain();
        tick_view(&mut b, &fight_view());
        stub.set_latest(Some(posture_decision(1, 1))); // hold
        tick_view(&mut b, &fight_view());
        // As live: `bot_task` calls on_death on the death frame BEFORE ticking it, so the policy
        // never sees health 0 (A1 recorded died = 0 % for exactly this reason).
        b.on_death();
        let o = stub.outcomes();
        assert!(o[0].1.died && !o[0].1.biased, "died; a hold never biases");
        // A level change: on_level_change, then the branch's on_death — not a death.
        let (mut b, stub) = fight_brain();
        tick_view(&mut b, &fight_view());
        stub.set_latest(Some(posture_decision(1, 0)));
        tick_view(&mut b, &fight_view());
        b.on_level_change();
        b.on_death();
        b.on_map_change("q2dm2");
        let o = stub.outcomes();
        assert_eq!(o.len(), 1);
        assert!(!o[0].1.died, "a level change is not a death");
    }

    #[test]
    fn jev_shadow_plays_exactly_as_jev_static_while_asking_and_journalling() {
        use crate::brains::{build_brain, build_brain_with_decider, BrainConfig, BrainKind};
        use crate::skill::BotSkill;
        let stub = Arc::new(StubDecider::default());
        let mut shadow = build_brain_with_decider(
            BrainKind::JevStatic,
            BotSkill::default(),
            BrainConfig::default(),
            None,
            None,
            None,
            Some(stub.clone()),
        );
        let mut plain = build_brain(
            BrainKind::JevStatic,
            BotSkill::default(),
            BrainConfig::default(),
            None,
            None,
            None,
        );
        shadow.on_map_change("q2dm1");
        plain.on_map_change("q2dm1");
        let run = |b: &mut Box<dyn Brain + Send>, view: &Worldview| {
            let mut nav = StubNav {
                pursue: Some(Vec3::new(200.0, 0.0, 0.0)),
                ..Default::default()
            };
            let out = b.tick(BrainContext {
                view,
                nav: Some(&mut nav),
                cm: None,
                dt: 0.1,
                ticks: 1,
                goal_override: None,
            });
            format!("{out:?} | {:?} | {}", nav.last_goal, b.status())
        };
        for i in 0..150 {
            if i == 1 {
                stub.set_latest(Some(posture_decision(1, 0))); // a disengage it must NOT act on
            }
            let view = fight_view_hp(if i < 60 { 100 } else { 60 });
            assert_eq!(run(&mut shadow, &view), run(&mut plain, &view), "tick {i}");
        }
        let trigs: Vec<Trigger> = stub.submitted().iter().map(|(_, a)| a.trig).collect();
        assert_eq!(
            trigs,
            vec![Trigger::Contact, Trigger::Damage],
            "the shadow asks like jev: on contact, and when hit (the stub does not pace)"
        );
        assert_eq!(stub.acks(), vec![(TicketId(1), ApplyOutcome::Shadowed)]);
        let o = stub.outcomes();
        assert_eq!(o.len(), 1);
        assert!(!o[0].1.biased, "a shadow never biases");
        assert_eq!(
            o[0].1.q3_retreat,
            Some(false),
            "and still records what q3 wanted"
        );
    }

    // ── Plan 74 T4: the pickup objective ─────────────────────────────────────────────────

    /// A corridor 0..4 along +x, 256 u apart; a rocket pad on node 3 (768 u) and an ammo box on
    /// node 1. The bot sees none of them.
    fn pad_map() -> BrainMap {
        let nodes = (0..5).map(|i| [i as f32 * 256.0, 0.0, 0.0]).collect();
        let adj = (0..5usize)
            .map(|i| {
                [i.checked_sub(1), (i < 4).then_some(i + 1)]
                    .into_iter()
                    .flatten()
                    .map(|j| (j, 256.0))
                    .collect()
            })
            .collect();
        let pad = |weapon, node: usize| MapItem {
            class: EntityClass::ItemWeapon,
            weapon,
            origin: Vec3::new(node as f32 * 256.0, 0.0, 0.0),
            nav_node: Some(node),
        };
        BrainMap {
            roam_nodes: vec![4],
            nav_graph: Arc::new(NavGraph::from_raw(nodes, adj)),
            roam_as_position: false,
            items: vec![
                pad(None, 1),
                pad(Some(crate::weapons::Weapon::RocketLauncher), 3),
            ],
        }
    }

    /// A healthy, armored bot holding the blaster at `x` on the corridor, optionally facing an
    /// enemy at 200 u; once `x > 0` the stocked rocket pad is in view.
    fn roam_view(x: f32, enemy: bool) -> Worldview {
        roam_view_with(x, enemy, "models/weapons/v_blast/tris.md2")
    }

    /// [`roam_view`] holding the weapon whose view model is `gun`.
    fn roam_view_with(x: f32, enemy: bool, gun: &str) -> Worldview {
        use q2proto::EntityState;
        let mut frame = Frame::default();
        frame.playerstate.gunindex = 1;
        frame.playerstate.stats[1] = 100;
        frame.playerstate.stats[3] = 50;
        frame.playerstate.stats[5] = 100;
        frame.playerstate.pmove.origin = [(x * 8.0) as i16, 0, 0];
        if enemy {
            frame.entities.push(EntityState {
                number: 9,
                origin: [x + 200.0, 0.0, 0.0],
                angles: [0.0, 180.0, 0.0],
                modelindex: 255,
                ..Default::default()
            });
        }
        if x > 0.0 {
            // Walking up the corridor the rocket pad is in PVS, and stocked — without it,
            // `ItemMemory` would (correctly) read the pad as taken inside its trust range.
            frame.entities.push(EntityState {
                number: 20,
                origin: [768.0, 0.0, 0.0],
                modelindex: 2,
                ..Default::default()
            });
        }
        let mut cs = ConfigStrings::default();
        cs.set(32 + 1, gun);
        cs.set(32 + 2, "models/weapons/g_rocket/tris.md2");
        Worldview::from_frame(&frame, &cs, 0)
    }

    fn pad_brain() -> (JevBrain, Arc<StubDecider>) {
        let stub = Arc::new(StubDecider::default());
        let mut b = JevBrain::new(Q3Character::default(), stub.clone());
        b.set_map(pad_map());
        b.on_map_change("q2dm1");
        (b, stub)
    }

    /// Tick `n` times at `view`; the goal the navigator was last given.
    fn roam(b: &mut JevBrain, view: &Worldview, n: u32) -> Option<NavGoal> {
        let mut nav = StubNav::default();
        for t in 0..n {
            b.tick(BrainContext {
                view,
                nav: Some(&mut nav),
                cm: None,
                dt: 0.1,
                ticks: t + 1,
                goal_override: None,
            });
        }
        nav.last_goal
    }

    fn objective_decision(ticket: TicketId, choice: Option<u8>) -> AppliedDecision {
        AppliedDecision {
            ticket,
            generation: 0,
            map: MapId::Q2dm1,
            objective: choice,
            posture: None,
        }
    }

    /// Run until the brain has asked the objective, then answer it with option `choice`.
    fn asked_and_answered(choice: Option<u8>) -> (JevBrain, Arc<StubDecider>, TicketId) {
        let (mut b, stub) = pad_brain();
        roam(&mut b, &roam_view(0.0, false), 25); // > RESPAWN_SETTLE_S
        let (ticket, _) = *stub.submitted().last().expect("an objective ask");
        stub.set_latest(Some(objective_decision(ticket, choice)));
        (b, stub, ticket)
    }

    #[test]
    fn jev_objective_asks_v2_with_pads_after_settling_and_seek_drives_to_the_pad() {
        use crate::jev::question::{objective_question_v2, Offered};
        use crate::jev::state::{DistBand, Field, FieldKey, PadFacts};
        let (mut b, stub) = pad_brain();
        roam(&mut b, &roam_view(0.0, false), 15);
        assert!(stub.submitted().is_empty(), "not before RESPAWN_SETTLE_S");
        roam(&mut b, &roam_view(0.0, false), 10);
        let sent = stub.submitted();
        assert_eq!(sent.len(), 1);
        let a = &sent[0].1;
        assert_eq!(
            a.trig,
            Trigger::MapChange,
            "the pending event, not a refresh"
        );
        let only_weapon = Offered {
            weapon: true,
            ..Offered::default()
        };
        assert_eq!(a.questions, vec![objective_question_v2(only_weapon)]);
        assert_eq!(
            a.state.get(FieldKey::Pads),
            Some(&Field::Pads(PadFacts {
                weapon: Some(DistBand::Mid),
                health: None,
                armor: None,
            })),
            "the rocket pad, 768 u of travel; the ammo box is not a weapon"
        );
        stub.set_latest(Some(objective_decision(sent[0].0, Some(0))));
        let goal = roam(&mut b, &roam_view(0.0, false), 3);
        assert_eq!(goal, Some(NavGoal::Position(Vec3::new(768.0, 0.0, 0.0))));
        assert_eq!(stub.acks(), vec![(sent[0].0, ApplyOutcome::Applied)]);
    }

    #[test]
    fn jev_objective_carry_on_and_off_ticket_leave_q3_roaming() {
        let (mut b, stub, t) = asked_and_answered(Some(1)); // carry_on
        let roam_goal = Some(NavGoal::Waypoint(4));
        assert_eq!(roam(&mut b, &roam_view(0.0, false), 2), roam_goal);
        assert!(b.policy().held_objective().is_none());
        assert_eq!(stub.acks(), vec![(t, ApplyOutcome::Applied)]);
        let (mut b, stub, t) = asked_and_answered(Some(2)); // only 2 options were offered
        assert_eq!(roam(&mut b, &roam_view(0.0, false), 2), roam_goal);
        assert_eq!(
            stub.acks(),
            vec![(t, ApplyOutcome::Rejected(BrainReject::OffTicketLabel))]
        );
    }

    #[test]
    fn jev_objective_holds_without_new_answers_and_releases_on_arrival() {
        let (mut b, stub, _) = asked_and_answered(Some(0));
        let pad = Some(NavGoal::Position(Vec3::new(768.0, 0.0, 0.0)));
        // Walk toward the pad at 32 u per tick (a standing bot would trip q3's stall detector,
        // which releases the objective — tested below).
        for i in 0..20 {
            let goal = roam(&mut b, &roam_view(i as f32 * 32.0, false), 1);
            assert_eq!(goal, pad, "held at tick {i}, no new answer needed");
        }
        assert_eq!(stub.submitted().len(), 1, "no re-ask while held");
        // Arrive: within ARRIVE_RADIUS of the pad (and the pad is now "seen" stocked or not —
        // either way the objective is done).
        roam(&mut b, &roam_view(768.0 - ARRIVE_RADIUS + 1.0, false), 1);
        assert!(b.policy().held_objective().is_none());
        let sent = stub.submitted();
        assert_eq!(sent.len(), 2, "the release re-asks at once");
        assert_eq!(sent[1].1.trig, Trigger::Released);
    }

    #[test]
    fn jev_objective_releases_on_stall_death_timeout_and_a_newer_objective_ticket() {
        use crate::brains::q3::GoalPolicy;
        let (mut b, ..) = asked_and_answered(Some(0));
        roam(&mut b, &roam_view(0.0, false), 1);
        b.policy_mut().on_stall();
        assert!(b.policy().held_objective().is_none(), "stall");

        let (mut b, ..) = asked_and_answered(Some(0));
        roam(&mut b, &roam_view(0.0, false), 1);
        b.on_death();
        assert!(b.policy().held_objective().is_none(), "death");

        let (mut b, ..) = asked_and_answered(Some(0));
        roam(&mut b, &roam_view(0.0, false), 1);
        assert!(b.policy().held_objective().is_some());
        roam(
            &mut b,
            &roam_view(0.0, false),
            (OBJECTIVE_MAX_S * 10.0) as u32 + 1,
        );
        assert!(b.policy().held_objective().is_none(), "timeout");

        // A newer ticket that asked the objective and came back gated off releases to roam.
        let (mut b, stub, t) = asked_and_answered(Some(0));
        roam(&mut b, &roam_view(0.0, false), 1);
        let newer = TicketId(t.0 + 1);
        b.policy_mut().record_ticket(
            newer,
            TicketEntry {
                objectives: vec![Some(pad_at(1, 768.0)), None],
                ..TicketEntry::default()
            },
        );
        stub.set_latest(Some(objective_decision(newer, None)));
        roam(&mut b, &roam_view(0.0, false), 1);
        assert!(
            b.policy().held_objective().is_none(),
            "newer objective ticket"
        );
    }

    #[test]
    fn jev_objective_survives_a_posture_only_ticket_and_is_never_asked_in_a_fight() {
        let (mut b, stub, _) = asked_and_answered(Some(0));
        roam(&mut b, &roam_view(0.0, false), 1);
        // Contact: a posture ask goes out; its answer carries objective None but did not ask it.
        roam(&mut b, &roam_view(0.0, true), 1);
        let (posture_ticket, a) = stub.submitted().last().cloned().expect("posture ask");
        assert_eq!(a.questions, vec![crate::jev::question::posture_question()]);
        stub.set_latest(Some(posture_decision(posture_ticket.0, 1)));
        stub.set_refresh_due(true);
        roam(&mut b, &roam_view(0.0, true), 30);
        assert!(
            b.policy().held_objective().is_some(),
            "a posture-only ticket never releases the objective"
        );
        let objective_asks = stub
            .submitted()
            .iter()
            .filter(|(_, a)| a.questions[0].key == crate::jev::question::QuestionKey::Objective)
            .count();
        assert_eq!(objective_asks, 1, "no objective ask with an enemy in view");
    }

    #[test]
    fn jev_objective_shadow_never_asks_it() {
        let stub = Arc::new(StubDecider::default());
        let mut b = JevBrain::new_shadow(Q3Character::default(), stub.clone());
        b.set_map(pad_map());
        b.on_map_change("q2dm1");
        stub.set_refresh_due(true);
        roam(&mut b, &roam_view(0.0, false), 40);
        assert!(stub.submitted().is_empty(), "the shadow asks posture only");
    }

    #[test]
    fn jev_objective_outcome_records_arrival_upgrade_and_closes_on_death_or_30s() {
        use crate::jev::question::{CARRY_ON, SEEK_WEAPON};
        let (mut b, stub, t) = asked_and_answered(Some(0)); // seek_weapon
        roam(&mut b, &roam_view(0.0, false), 1); // resolves: the window opens
        for i in 1..=22 {
            roam(&mut b, &roam_view(i as f32 * 32.0, false), 1); // reaches 704 u: arrived
        }
        let rocket = "models/weapons/v_rocket/tris.md2";
        roam(&mut b, &roam_view_with(768.0, false, rocket), 1); // picked it up
        assert!(stub.outcomes().is_empty(), "still open");
        b.on_death();
        let (ot, o) = *stub.outcomes().last().expect("closed on death");
        assert_eq!(ot, t);
        assert!(o.died);
        let ob = o.objective.expect("an objective outcome");
        assert_eq!(
            (ob.applied, ob.rule),
            (Some(SEEK_WEAPON), Some(SEEK_WEAPON))
        );
        assert!(ob.arrived && ob.upgraded, "{ob:?}");
        assert!(
            ob.secs_to_arrive.is_some_and(|s| s > 0.0 && s < 3.0),
            "{ob:?}"
        );

        // carry_on: nothing to walk to; the window closes by itself after 30 s of brain time.
        let (mut b, stub, _) = asked_and_answered(Some(1));
        roam(
            &mut b,
            &roam_view(0.0, false),
            (OBJECTIVE_MAX_S * 10.0) as u32 + 2,
        );
        let (_, o) = *stub.outcomes().last().expect("closed at 30 s");
        assert!(!o.died && o.secs >= 30.0);
        let ob = o.objective.expect("objective");
        assert_eq!(ob.applied, Some(CARRY_ON));
        assert!(!ob.arrived && o.q3_retreat.is_none() && !o.biased);
    }

    #[test]
    fn jev_rule_applies_its_pick_at_ask_time_and_only_shadows_the_model() {
        let stub = Arc::new(StubDecider::default());
        let mut b = JevBrain::new_rule(Q3Character::default(), stub.clone());
        b.set_map(pad_map());
        b.on_map_change("q2dm1");
        assert!(b.policy().is_rule());
        let rocket = Some(NavGoal::Position(Vec3::new(768.0, 0.0, 0.0)));
        // Blaster + a rocket pad: the rule seeks the weapon the moment the ask is accepted.
        assert_eq!(roam(&mut b, &roam_view(0.0, false), 25), rocket);
        let (t, a) = stub.submitted()[0].clone();
        assert_eq!(
            a.questions[0].key,
            crate::jev::question::QuestionKey::Objective,
            "the model is still asked"
        );
        // The model disagrees (carry_on): journalled as a shadow, the rule's goal stands.
        stub.set_latest(Some(objective_decision(t, Some(1))));
        assert_eq!(roam(&mut b, &roam_view(0.0, false), 1), rocket);
        assert_eq!(stub.acks(), vec![(t, ApplyOutcome::Shadowed)]);
        // Posture is live, exactly as jev's.
        roam(&mut b, &roam_view(0.0, true), 1);
        let (pt, _) = stub.submitted().last().cloned().expect("posture ask");
        stub.set_latest(Some(posture_decision(pt.0, 1)));
        roam(&mut b, &roam_view(0.0, true), 1);
        assert_eq!(stub.acks().last(), Some(&(pt, ApplyOutcome::Applied)));
    }

    #[test]
    fn jev_brain_status_delegates_verbatim() {
        let b = JevBrain::new(Q3Character::default(), Arc::new(StubDecider::default()));
        assert_eq!(b.status(), "seek-ltg", "no jev: prefix");
    }
}
