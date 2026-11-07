//! # brain::brains::q3::policy — the `GoalPolicy` seam (Plan 72 T6)
//!
//! A type parameter on [`Q3Brain`](super::Q3Brain) that lets a wrapper brain (Plan 72's `jev`)
//! bias q3's **high-level** choices without forking q3:
//!
//! - [`GoalPolicy::objective`] may replace the goal of q3's roam-fallback arm (the `SeekLtg`
//!   path-follow) — and nothing else; battle/retreat goals stay q3's.
//! - [`GoalPolicy::posture`] may **only add caution**: a [`Posture::disengage`] forces
//!   `wants_retreat` on **and** `wants_chase` off, and lets the sneak-past veto skip a distant,
//!   unaware enemy. It can never add aggression.
//! - [`GoalPolicy::on_stall`] reports a stuck-recovery escalation so a pinned goal is released.
//!
//! Every method defaults to "no opinion", and [`NullPolicy`] is that default — so
//! `Q3Brain<NullPolicy>` (which `Q3Brain` means in type position) behaves byte-for-byte as q3 did
//! before this seam existed; `q3_parity_golden` in `super::tests` holds it to that.
//!
//! **Why a disengage must also clear chase.** q3's own heuristics are mutually exclusive
//! (`wants_to_retreat` is `aggression < threshold`, `wants_to_chase` is `>`), and its node FSM
//! loops within a tick until no transition fires. Retreat and chase both true would cycle
//! Fight → Retreat → Chase → Fight until the node-switch guard trips, leaving the bot in a Chase
//! that never fires. The merge in `Q3Brain::tick` therefore sets both.

use glam::Vec3;

use crate::nav::NavGoal;
use crate::perception::Worldview;
use crate::q3char::Q3Character;

/// What a policy may see when asked for an objective (the `SeekLtg` roam-fallback arm).
pub struct GoalQuery<'a> {
    /// This frame's perceived world.
    pub view: &'a Worldview,
    /// Monotonic tick counter since connect (the same value q3's roam cursor uses).
    pub ticks: u32,
    /// Our position this frame.
    pub pos: Vec3,
}

/// What a policy may see when asked for a posture. Asked **once per tick**, before enemy
/// selection, so both posture sites see one answer.
pub struct PostureQuery<'a> {
    /// This frame's perceived world.
    pub view: &'a Worldview,
    /// The bot's Q3 character (aggression bias, alertness, …).
    pub ch: &'a Q3Character,
}

/// A policy's engagement bias. The default is "no opinion" — q3's heuristics decide alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Posture {
    /// Add caution: force `wants_retreat`, clear `wants_chase`, and let the sneak-past veto skip
    /// a distant enemy that is not looking at us. There is deliberately no aggressive variant.
    pub disengage: bool,
}

/// The seam q3 consults for its high-level choices. Every method defaults to "no opinion".
pub trait GoalPolicy {
    /// A replacement goal for the roam-fallback arm only. `None` ⇒ q3's own `roam_goal`.
    fn objective(&mut self, _q: &GoalQuery) -> Option<NavGoal> {
        None
    }

    /// The engagement bias for this tick (see [`Posture`]). Called once per tick.
    fn posture(&mut self, _q: &PostureQuery) -> Posture {
        Posture::default()
    }

    /// Stuck recovery escalated (`BackOffThenRepath` / `BoxedIn`) — release any pinned goal.
    fn on_stall(&mut self) {}
}

/// The default policy: no opinion on anything — today's q3 behaviour.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullPolicy;

impl GoalPolicy for NullPolicy {}
