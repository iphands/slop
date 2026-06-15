//! Danger / popularity heatmap — a per-bot runtime cost overlay on the static
//! nav graph (Plan 08).
//!
//! The nav topology (Plan 05) is frozen; this overlay breathes per-node weights
//! learned from what the server sends us:
//! - **Danger**: bumped where we die or take damage; decays exponentially
//!   (`TAU_DANGER` ~45 s) so a recently-hot kill-zone cools off.
//! - **Popularity**: an EMA of observed enemy presence per node (slower,
//!   minute-scale) — busy lanes where fights/items happen.
//!
//! A\* then costs an edge as `base_len + W_d·danger − W_p·popularity`, routing
//! around death-traps and toward hot lanes. Per-bot (no shared mutable state —
//! AGENTS.md §Concurrency): each bot's heatmap reflects its own PVS-limited
//! observations, exactly like a real player's mental map.

/// Danger time-constant: a node's danger decays to ~37% over this many seconds.
/// Short-term "this place is hot right now." (`distilled/eraser.md` §10/§13-D.)
pub const TAU_DANGER: f32 = 45.0;
/// Popularity EMA rate (per second). Slower → minute-scale "busy lane."
pub const POP_K: f32 = 0.08;
/// Bump applied to a node's danger on our own death there.
pub const DANGER_BUMP_DEATH: f32 = 1.0;
/// Smaller bump when we merely take damage near a node (we're under fire).
pub const DANGER_BUMP_DAMAGE: f32 = 0.25;
/// Cap per-node danger so a single spot can't become an infinite wall.
pub const DANGER_MAX: f32 = 8.0;

/// Per-node danger + popularity overlay for a nav graph of `node_count` nodes.
#[derive(Debug, Clone)]
pub struct Heatmap {
    danger: Vec<f32>,
    popularity: Vec<f32>,
    /// Tracked for diagnostics (max danger seen).
    max_danger: f32,
}

impl Heatmap {
    pub fn new(node_count: usize) -> Self {
        Self {
            danger: vec![0.0; node_count],
            popularity: vec![0.0; node_count],
            max_danger: 0.0,
        }
    }

    pub fn node_count(&self) -> usize {
        self.danger.len()
    }

    pub fn danger(&self, node: usize) -> f32 {
        self.danger.get(node).copied().unwrap_or(0.0)
    }

    pub fn popularity(&self, node: usize) -> f32 {
        self.popularity.get(node).copied().unwrap_or(0.0)
    }

    /// Record our own death at `node` — a high-confidence danger signal.
    pub fn record_death(&mut self, node: usize) {
        self.bump(node, DANGER_BUMP_DEATH);
    }

    /// Record taking damage near `node` — weaker, "under fire here" signal.
    pub fn record_damage(&mut self, node: usize) {
        self.bump(node, DANGER_BUMP_DAMAGE);
    }

    fn bump(&mut self, node: usize, amount: f32) {
        if let Some(d) = self.danger.get_mut(node) {
            *d = (*d + amount).min(DANGER_MAX);
            if *d > self.max_danger {
                self.max_danger = *d;
            }
        }
    }

    /// Sample enemy presence at `node`: `present=true` means an enemy was seen
    /// there this tick. Drives the popularity EMA toward 1 (present) or 0.
    pub fn sample_presence(&mut self, node: usize, present: bool, dt: f32) {
        if let Some(p) = self.popularity.get_mut(node) {
            let target = if present { 1.0 } else { 0.0 };
            // p += (target - p) * (1 - exp(-K*dt))
            let alpha = 1.0 - (-POP_K * dt).exp();
            *p += (target - *p) * alpha;
        }
    }

    /// Advance decay over `dt` seconds. Danger cools exponentially. Popularity is
    /// **not** decayed here — its EMA in `sample_presence` already converges back
    /// to 0 when a node stops being sampled (a quieted lane cools off on its own).
    pub fn decay(&mut self, dt: f32) {
        let danger_factor = (-dt / TAU_DANGER).exp();
        for d in &mut self.danger {
            *d *= danger_factor;
        }
    }

    /// Build the per-node additive cost overlay used by risk-weighted A\*:
    /// `W_d·danger − W_p·popularity`. High-skill bots weight danger more
    /// (risk-averse); aggressive bots weight popularity more (seek action).
    pub fn cost_overlay(&self, w_danger: f32, w_pop: f32) -> Vec<f32> {
        self.danger
            .iter()
            .zip(self.popularity.iter())
            .map(|(d, p)| w_danger * d - w_pop * p)
            .collect()
    }

    /// Highest danger value ever recorded (diagnostics).
    pub fn max_danger_seen(&self) -> f32 {
        self.max_danger
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn death_bumps_danger_capped() {
        let mut h = Heatmap::new(4);
        for _ in 0..100 {
            h.record_death(1);
        }
        assert_eq!(h.danger(1), DANGER_MAX, "danger is capped");
        assert_eq!(h.danger(0), 0.0, "other nodes untouched");
    }

    #[test]
    fn danger_decays_exponentially() {
        let mut h = Heatmap::new(2);
        h.record_death(0);
        let initial = h.danger(0);
        // After one time-constant, danger ~ 37%.
        h.decay(TAU_DANGER);
        assert!(
            (h.danger(0) - initial * (-1.0_f32).exp()).abs() < 0.01,
            "decayed to ~37%, got {}",
            h.danger(0)
        );
    }

    #[test]
    fn popularity_ema_tracks_presence() {
        let mut h = Heatmap::new(2);
        // Sample "present" repeatedly → popularity rises toward 1.
        for _ in 0..1000 {
            h.sample_presence(0, true, 0.1);
        }
        assert!(
            h.popularity(0) > 0.95,
            "present node trends to 1, got {}",
            h.popularity(0)
        );
        // Then "absent" repeatedly → falls toward 0.
        for _ in 0..1000 {
            h.sample_presence(0, false, 0.1);
        }
        assert!(
            h.popularity(0) < 0.05,
            "absent node trends to 0, got {}",
            h.popularity(0)
        );
    }

    #[test]
    fn cost_overlay_combines_both_signals() {
        let mut h = Heatmap::new(2);
        h.record_death(0); // danger at node 0
        for _ in 0..100 {
            h.sample_presence(1, true, 0.1);
        } // popularity at node 1
        let overlay = h.cost_overlay(10.0, 5.0);
        assert!(overlay[0] > 0.0, "danger node has positive cost");
        assert!(
            overlay[1] < 0.0,
            "popular node has negative (preferred) cost"
        );
    }
}
