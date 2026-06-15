//! Runtime observation → heatmap ingestion (Plan 08 T1/T4).
//!
//! Owns one bot's [`Heatmap`] plus the bookkeeping to feed it from what the
//! server actually sends us, then exposes a cost overlay for risk-weighted A\*
//! (Plan 08 T3). **Per-bot, never shared** (AGENTS.md §Concurrency): each bot's
//! heatmap reflects only its own PVS-limited observations, exactly like a real
//! player's mental map — we can only fear or credit places we've located.
//!
//! Signals, all PVS-honest:
//! - **Self death/damage** → our own node (highest confidence — we know our
//!   origin exactly; the brain tick detects health hits).
//! - **Enemy presence** → popularity at each visible enemy's nearest node, and
//!   a `name → last-known-node` cache that seeds obituary attribution.
//! - **Obituary prints** → a named victim's death bumps their last-known node,
//!   but only if we've *observed* them recently (T4 omniscience-creep guard).
//!
//! Composes with Plan 07 T3's tactical projectile dodge: that is frame-scale
//! (dodge an imminent rocket); this is minute-scale (route around a kill-zone).

use std::collections::HashMap;
use std::sync::Arc;

use client::parse::ConfigStrings;
use glam::Vec3;
use world::NavGraph;

use crate::heatmap::Heatmap;
use crate::perception::{player_name, Worldview};

/// How many frames a cached player-node stays "trusted" for obituary
/// attribution. A touch longer than `perception::STALE_THRESHOLD` (~1 s): once
/// we've not seen a player for this long we won't pin a fresh death to their
/// stale last-known node (PVS-honesty, T4).
const PLAYER_NODE_TTL: i32 = 20; // ~2 s at 10 Hz

/// Compact heatmap state for periodic debug logging (Plan 08 T4). Cheap to build
/// each tick; the brain logs it on a cadence and a future tools binary can render
/// it as a "danger map" overlay.
#[derive(Debug, Clone, Default)]
pub struct HeatmapSnapshot {
    /// Nav nodes under observation.
    pub node_count: usize,
    /// Highest danger value ever seen.
    pub max_danger: f32,
    /// Sum of danger across all nodes (overall "heat").
    pub total_danger: f32,
    /// `(node, danger)` for the few hottest nodes, descending.
    pub hot_nodes: Vec<(usize, f32)>,
}

/// One parsed Q2 death message (`ClientObituary`, `p_hud.c`). The server prints
/// these as `svc_print` text with **no coordinates**, so we resolve names by
/// matching against the known player set rather than parsing grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Obituary {
    /// The player who died.
    pub victim: String,
    /// The player who killed them, if any (`None` = suicide/environment).
    pub killer: Option<String>,
}

/// Parse a `svc_print` line into an [`Obituary`] given the set of known player
/// names. Death lines are free-form ("bot1 was railed by bot2", "bot1 ate bot2's
/// rocket", "bot1 cratered"); the earliest-occurring known name is the victim,
/// the next is the killer. Returns `None` for non-death prints or when no known
/// name appears (we can't attribute a death to someone we've never observed).
///
/// Matching is name-substring at a word boundary so "Al" doesn't match "Alpha"
/// and "bot1" doesn't match "bot10".
pub fn parse_obituary(text: &str, names: &[&str]) -> Option<Obituary> {
    // Earliest first occurrence of each known name (stable sort keeps name order
    // as a tiebreak).
    let mut hits: Vec<(usize, &str)> = Vec::new();
    for &name in names {
        if name.is_empty() {
            continue;
        }
        if let Some(pos) = find_name(text, name) {
            hits.push((pos, name));
        }
    }
    if hits.is_empty() {
        return None;
    }
    hits.sort_by_key(|(p, _)| *p);
    let victim = hits[0].1.to_string();
    let killer = hits.get(1).map(|(_, n)| n.to_string());
    Some(Obituary { victim, killer })
}

/// First occurrence of `name` in `text` at an alphanumeric/underscore boundary.
fn find_name(text: &str, name: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let nb = name.as_bytes();
    if nb.is_empty() || nb.len() > bytes.len() {
        return None;
    }
    let mut from = 0;
    while from + nb.len() <= bytes.len() {
        let idx = text[from..].find(name)?;
        let start = from + idx;
        let end = start + nb.len();
        let before_ok = start == 0 || !is_name_char(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_name_char(bytes[end]);
        if before_ok && after_ok {
            return Some(start);
        }
        from = start + 1;
    }
    None
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Per-bot heatmap ingestion driver: feeds a [`Heatmap`] from observed events
/// and exposes the risk-weighted A\* cost overlay. Construct one per bot after
/// its nav graph loads.
pub struct HeatmapObserver {
    heatmap: Heatmap,
    graph: Arc<NavGraph>,
    /// `player name → (last-known node, last-seen frame)`. Seeds obituary
    /// attribution; entries age out by `PLAYER_NODE_TTL`.
    player_nodes: HashMap<String, (usize, i32)>,
    /// Player names we've ever observed (for obituary victim matching).
    known_names: Vec<String>,
}

impl HeatmapObserver {
    /// New observer over `graph` (sized to its node count). `our_name` seeds the
    /// known-name set so our own obituaries resolve (though self-death is
    /// normally attributed via health in the brain tick).
    pub fn new(graph: Arc<NavGraph>, our_name: &str) -> Self {
        let mut known_names = Vec::new();
        if !our_name.is_empty() {
            known_names.push(our_name.to_string());
        }
        Self {
            heatmap: Heatmap::new(graph.node_count()),
            graph,
            player_nodes: HashMap::new(),
            known_names,
        }
    }

    /// Read-only access to the underlying heatmap (diagnostics).
    pub fn heatmap(&self) -> &Heatmap {
        &self.heatmap
    }

    /// Advance time by `dt` seconds: decay danger + popularity. Popularity cools
    /// uniformly here, so [`Self::sample_presence`] only needs to *heat* present
    /// nodes — a quieted lane fades on its own.
    pub fn tick(&mut self, dt: f32) {
        self.heatmap.decay(dt);
    }

    /// Our own death at `origin` — the highest-confidence danger signal (we know
    /// our exact position). Records a full danger bump at the nearest node.
    pub fn on_self_death(&mut self, origin: Vec3) {
        if let Some(node) = self.nearest_node(origin) {
            self.heatmap.record_death(node);
            tracing::debug!(node, "heatmap: recorded own death");
        }
    }

    /// We took damage near `origin` — a weaker "under fire here" signal. Records
    /// a smaller danger bump so repeated incoming fire still marks a kill-zone.
    pub fn on_self_damage(&mut self, origin: Vec3) {
        if let Some(node) = self.nearest_node(origin) {
            self.heatmap.record_damage(node);
        }
    }

    /// Sample enemy presence: bump popularity at each visible enemy's nearest
    /// node and refresh the `name → node` cache used for obituary attribution.
    /// `dt` is seconds since the last sample.
    pub fn sample_presence(&mut self, view: &Worldview, cs: &ConfigStrings, dt: f32, frame: i32) {
        for e in view.enemies() {
            let Some(node) = self.nearest_node(e.origin) else {
                continue;
            };
            self.heatmap.sample_presence(node, true, dt);
            if let Some(name) = player_name(cs, e.entity_number) {
                self.player_nodes.insert(name.clone(), (node, frame));
                if !self.known_names.iter().any(|n| n == &name) {
                    self.known_names.push(name);
                }
            }
        }
    }

    /// A server print, possibly an obituary. Attribute a named victim's death to
    /// their last-known node — but only if we've *observed* them recently (T4:
    /// we can't fear a place we've never located). Our own deaths are handled via
    /// health (exact origin) in the brain tick, so self-victim prints are skipped
    /// to avoid double-counting.
    pub fn on_print(&mut self, text: &str, our_name: &str, frame: i32) {
        let names: Vec<&str> = self.known_names.iter().map(String::as_str).collect();
        let Some(obit) = parse_obituary(text, &names) else {
            return;
        };
        if !our_name.is_empty() && obit.victim == our_name {
            return; // self-death already recorded via health
        }
        let Some(&(node, last_frame)) = self.player_nodes.get(&obit.victim) else {
            return; // never observed this player → no node to fear
        };
        if frame.saturating_sub(last_frame) > PLAYER_NODE_TTL {
            return; // their last-known node is too stale to trust
        }
        self.heatmap.record_death(node);
        tracing::debug!(
            victim = %obit.victim,
            killer = ?obit.killer,
            node,
            "heatmap: attributed enemy death to last-known node"
        );
    }

    /// Build the per-node cost overlay for risk-weighted A\*:
    /// `W_d·danger − W_p·popularity`. High-skill bots weight danger more
    /// (risk-averse); aggressive bots weight popularity more (seek action).
    pub fn cost_overlay(&self, w_danger: f32, w_pop: f32) -> Vec<f32> {
        self.heatmap.cost_overlay(w_danger, w_pop)
    }

    /// A compact snapshot for periodic debug logging / a danger-map overlay
    /// (Plan 08 T4). `hot_k` caps how many hot nodes to report.
    pub fn snapshot(&self, hot_k: usize) -> HeatmapSnapshot {
        let hm = &self.heatmap;
        HeatmapSnapshot {
            node_count: hm.node_count(),
            max_danger: hm.max_danger_seen(),
            total_danger: hm.total_danger(),
            hot_nodes: hm.hot_nodes(hot_k),
        }
    }

    fn nearest_node(&self, origin: Vec3) -> Option<usize> {
        self.graph.nearest(&[origin.x, origin.y, origin.z])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::EntityClass;
    use client::parse::ConfigStrings;
    use glam::Vec3;
    use q2proto::{EntityState, Frame, PlayerState};

    // ---- parse_obituary (pure) ----

    #[test]
    fn obituary_finds_victim_and_killer() {
        let obit = parse_obituary("bot1 was railed by bot2", &["bot1", "bot2"]).unwrap();
        assert_eq!(obit.victim, "bot1");
        assert_eq!(obit.killer.as_deref(), Some("bot2"));
    }

    #[test]
    fn obituary_suicide_one_name() {
        // "bot1 cratered" — only one known name → suicide (no killer).
        let obit = parse_obituary("bot1 cratered", &["bot1", "bot2"]).unwrap();
        assert_eq!(obit.victim, "bot1");
        assert!(obit.killer.is_none());
    }

    #[test]
    fn obituary_unknown_text_returns_none() {
        // No known name in the text → not attributable.
        assert!(parse_obituary("bot1 was railed by bot2", &["bot9"]).is_none());
        // Non-death print → None.
        assert!(parse_obituary("Server: timelimit hit", &["bot1"]).is_none());
    }

    #[test]
    fn obituary_word_boundary() {
        // "bot1" must not match inside "bot10"; the victim is bot10, killer bot1.
        let obit = parse_obituary("bot10 was railed by bot1", &["bot1", "bot10"]).unwrap();
        assert_eq!(obit.victim, "bot10");
        assert_eq!(obit.killer.as_deref(), Some("bot1"));
    }

    // ---- HeatmapObserver (integration over a tiny nav graph + worldview) ----

    /// Three collinear nodes at x = 0, 100, 200.
    fn tiny_graph() -> Arc<NavGraph> {
        Arc::new(NavGraph::from_raw(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [200.0, 0.0, 0.0]],
            vec![
                vec![(1, 100.0)],
                vec![(0, 100.0), (2, 100.0)],
                vec![(1, 100.0)],
            ],
        ))
    }

    #[test]
    fn self_death_records_at_nearest_node() {
        let mut obs = HeatmapObserver::new(tiny_graph(), "me");
        // Die near node 1 (x≈100).
        obs.on_self_death(Vec3::new(95.0, 0.0, 0.0));
        assert!(obs.heatmap().danger(1) > 0.0, "node 1 gets danger");
        assert_eq!(obs.heatmap().danger(0), 0.0, "node 0 untouched");
        assert_eq!(obs.heatmap().danger(2), 0.0, "node 2 untouched");
    }

    #[test]
    fn snapshot_reports_hot_nodes_and_totals() {
        let mut obs = HeatmapObserver::new(tiny_graph(), "me");
        obs.on_self_death(Vec3::new(100.0, 0.0, 0.0)); // node 1
        obs.on_self_death(Vec3::new(200.0, 0.0, 0.0)); // node 2
        let snap = obs.snapshot(3);
        assert_eq!(snap.node_count, 3);
        assert!(snap.total_danger > 1.5, "total danger ~2.0");
        assert_eq!(snap.hot_nodes.len(), 2);
        // Hottest first (descending).
        assert!(snap.hot_nodes[0].1 >= snap.hot_nodes[1].1);
    }

    #[test]
    fn presence_seeds_player_node_then_obituary_attributes() {
        // Place an enemy "Foe" (entity_number=2 → CS_PLAYERSKINS+1) at node 2.
        let cs = enemy_skin_configstrings(2, "Foe");
        let view = view_with_enemy_at(Vec3::new(200.0, 0.0, 0.0));
        let mut obs = HeatmapObserver::new(tiny_graph(), "me");

        // Seeing Foe at node 2 heats popularity and caches name→node.
        obs.sample_presence(&view, &cs, 0.1, 100);
        assert!(obs.heatmap().popularity(2) > 0.0, "node 2 popular");

        // Obituary: Foe dies. Attributed to Foe's last-known node (2).
        obs.on_print("Foe was railed by someone", "me", 100);
        assert!(
            obs.heatmap().danger(2) > 0.0,
            "Foe's death bumps node 2 danger"
        );
    }

    #[test]
    fn obituary_ignores_unobserved_player() {
        // We've never seen "Stranger", so their death is a PVS-honest no-op.
        let mut obs = HeatmapObserver::new(tiny_graph(), "me");
        obs.on_print("Stranger was railed by bot2", "me", 100);
        for n in 0..3 {
            assert_eq!(obs.heatmap().danger(n), 0.0, "node {n} untouched");
        }
    }

    #[test]
    fn obituary_ignores_self_victim() {
        // Self-death is handled via health; the obituary is a no-op here.
        let mut obs = HeatmapObserver::new(tiny_graph(), "me");
        obs.on_print("me was railed by bot2", "me", 100);
        for n in 0..3 {
            assert_eq!(obs.heatmap().danger(n), 0.0, "node {n} untouched");
        }
    }

    #[test]
    fn obituary_ignores_stale_player() {
        let cs = enemy_skin_configstrings(2, "Foe");
        let view = view_with_enemy_at(Vec3::new(200.0, 0.0, 0.0));
        let mut obs = HeatmapObserver::new(tiny_graph(), "me");
        obs.sample_presence(&view, &cs, 0.1, 100);
        // A death reported long after we last saw Foe → too stale to trust.
        obs.on_print("Foe was railed by bot2", "me", 100 + PLAYER_NODE_TTL + 1);
        assert_eq!(
            obs.heatmap().danger(2),
            0.0,
            "stale player's death not attributed"
        );
    }

    #[test]
    fn popularity_cools_after_presence_stops() {
        let cs = enemy_skin_configstrings(2, "Foe");
        let view = view_with_enemy_at(Vec3::new(200.0, 0.0, 0.0));
        let mut obs = HeatmapObserver::new(tiny_graph(), "me");
        // Heat node 2.
        for _ in 0..200 {
            obs.sample_presence(&view, &cs, 0.1, 1);
            obs.tick(0.1);
        }
        let hot = obs.heatmap().popularity(2);
        assert!(hot > 0.5, "node 2 heated, got {hot}");
        // Stop seeing the enemy; let decay cool it.
        let empty = Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0);
        for _ in 0..5000 {
            obs.sample_presence(&empty, &ConfigStrings::default(), 0.1, 1);
            obs.tick(0.1);
        }
        assert!(
            obs.heatmap().popularity(2) < hot * 0.5,
            "popularity cooled after presence stopped"
        );
    }

    // ---- test helpers ----

    /// ConfigStrings with `name\NAME\` set for the given client entity_number.
    fn enemy_skin_configstrings(entity_number: i32, name: &str) -> ConfigStrings {
        let mut cs = ConfigStrings::default();
        let idx = crate::perception::CS_PLAYERSKINS + (entity_number - 1) as usize;
        cs.set(idx, format!("name\\{name}\\skin\\male/grunt"));
        cs
    }

    /// A worldview whose only entity is an enemy player (modelindex 255) at `pos`
    /// with entity_number 2.
    fn view_with_enemy_at(pos: Vec3) -> Worldview {
        let mut frame = Frame::default();
        let mut ps = PlayerState::default();
        // Self at the origin so we're not co-located with the enemy.
        ps.pmove.origin = [0, 0, 0];
        frame.playerstate = ps;
        let enemy = EntityState {
            number: 2,
            origin: [pos.x, pos.y, pos.z],
            modelindex: 255,
            ..Default::default()
        };
        frame.entities.push(enemy);
        let cs = ConfigStrings::default();
        let mut view = Worldview::from_frame(&frame, &cs, 0);
        // from_frame classifies modelindex 255 as EnemyPlayer already; ensure it.
        for e in view.entities_mut() {
            if e.entity_number == 2 {
                e.class = EntityClass::EnemyPlayer;
            }
        }
        view
    }
}
