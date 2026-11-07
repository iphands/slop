//! # brain::brains::jev::policy — `JevPolicy`, the `GoalPolicy` the jev brain plugs into q3
//!
//! T7 builds the shell: it owns the bot's `DecisionSource` handle, its own copies of the map
//! facts q3 discards (`items`) or keeps privately (`roam_nodes`, `nav_graph`), and the **inert**
//! flag. While inert (`goal_override` is set — the `spawn-to-*` harnesses) it has no opinion on
//! anything and never asks. The objective (T10) and posture (T12) logic land on top of this.

use std::sync::Arc;

use world::NavGraph;

use crate::brains::core::MapItem;
use crate::brains::q3::{GoalPolicy, GoalQuery, Posture, PostureQuery};
use crate::jev::source::DecisionSource;
use crate::nav::NavGoal;

/// The jev brain's policy. See the module doc.
pub struct JevPolicy {
    source: Arc<dyn DecisionSource>,
    inert: bool,
    roam_nodes: Vec<usize>,
    nav_graph: Option<Arc<NavGraph>>,
    items: Vec<MapItem>,
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
}

impl GoalPolicy for JevPolicy {
    fn objective(&mut self, _q: &GoalQuery) -> Option<NavGoal> {
        // T10 adds candidate generation and asking; inert or not, there is no opinion yet.
        None
    }

    fn posture(&mut self, _q: &PostureQuery) -> Posture {
        // T12 adds the posture question. Never biased while inert.
        Posture::default()
    }
}
