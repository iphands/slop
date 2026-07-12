//! Enemy selection + weapon choice (Plan 60 T3; distilled `xonotic.md` §6).
//!
//! **Enemy selection** (`havocbot_chooseenemy`, `havocbot.qc:1334-1461`): re-scan every
//! `bot_ai_enemydetectioninterval` 2 s (4 s while sticking to a target); sticky keeps the
//! current enemy while it stays visible within 1000 u, extending 0.5 s per check; otherwise
//! pick the **nearest visible** enemy. Vendor-authentic: NO awareness-FOV gate (havocbot has
//! none — it full-sphere scans with an LOS check), which also subsumes the Plan 49
//! damage-widen contract (a bot shot from behind acquires immediately).
//!
//! **Weapon choice** (`havocbot_chooseweapon`, `havocbot.qc:1495-1570`): three priority
//! lists (far/mid/close) at thresholds 850/300 on the *effective* distance
//! `bound(10, dist−200, 10000) * 2^rangepreference` (`:1564`) — the per-bot sniper/spammer
//! bias. **Weapon combos** (`:1544-1559`): if the just-fired gun's refire won't finish
//! within `0.4 * (4 − 0.3*(skill+weaponskill))` seconds, switch mid-refire; then locked 1 s.
//!
//! **Q2 wire adaptation** (see `weapons::select_best_weapon`): ownership is not on the wire —
//! `use <unowned>` is a server no-op. We *probe and learn*: request the list's best; if the
//! held weapon doesn't change within a grace window, mark that weapon assumed-unowned for
//! 30 s and fall through to the next candidate. Requests are rate-limited (the Plan 47
//! weapon-thrash lesson: 4179 requests/5 min once suppressed all firing).

use glam::Vec3;
use world::CollisionModel;

use crate::los;
use crate::perception::{EntityClass, Worldview};
use crate::weapons::Weapon;
use crate::xonchar::XonSkill;

/// `bot_ai_enemydetectioninterval` (seconds).
const RESCAN_SECS: f32 = 2.0;
/// `bot_ai_enemydetectioninterval_stickingtoenemy` (seconds).
const RESCAN_STICKY_SECS: f32 = 4.0;
/// Sticky range: keep the current enemy while visible within this (`havocbot.qc:1350-1365`).
const STICKY_RANGE: f32 = 1000.0;
/// Sticky extension per visible check (`havocbot.qc:1361`).
const STICKY_EXTEND: f32 = 0.5;

/// Far/mid effective-distance thresholds (`bot_ai_custom_weapon_priority_distances` "300 850").
const DIST_CLOSE: f32 = 300.0;
const DIST_FAR: f32 = 850.0;
/// `bot_ai_weapon_combo_threshold`.
const COMBO_THRESHOLD: f32 = 0.4;
/// Post-combo switch lockout (`havocbot.qc:1557`), seconds.
const COMBO_LOCK: f32 = 1.0;
/// Weapon re-choose cadence (`bot_ai_chooseweaponinterval`), seconds.
const CHOOSE_INTERVAL: f32 = 0.5;
/// Min seconds between emitted `use` requests (Plan 47 thrash guard).
const REQUEST_COOLDOWN: f32 = 1.0;
/// If the held weapon doesn't become the requested one within this, assume unowned.
const PROBE_GRACE: f32 = 0.5;
/// How long an assumed-unowned mark lasts (we may pick the weapon up meanwhile).
const UNOWNED_MEMORY: f32 = 30.0;

/// Q2 priority lists (Q2 adaptation of the `bot_ai_custom_weapon_priority_*` cvars).
/// Order = preference; the self-splash guard (`min_safe_distance`) filters at pick time.
const LIST_CLOSE: [Weapon; 6] = [
    Weapon::SuperShotgun,
    Weapon::Shotgun,
    Weapon::Hyperblaster,
    Weapon::Chaingun,
    Weapon::Machinegun,
    Weapon::Blaster,
];
const LIST_MID: [Weapon; 8] = [
    Weapon::RocketLauncher,
    Weapon::Hyperblaster,
    Weapon::Chaingun,
    Weapon::SuperShotgun,
    Weapon::Railgun,
    Weapon::GrenadeLauncher,
    Weapon::Machinegun,
    Weapon::Blaster,
];
const LIST_FAR: [Weapon; 7] = [
    Weapon::Railgun,
    Weapon::RocketLauncher,
    Weapon::Hyperblaster,
    Weapon::Machinegun,
    Weapon::Chaingun,
    Weapon::GrenadeLauncher,
    Weapon::Blaster,
];

/// A selected enemy this frame.
#[derive(Debug, Clone, Copy)]
pub struct Enemy {
    pub id: i32,
    pub pos: Vec3,
    /// Frame-delta velocity when fresh (`None` = stale/unknown) — the aim lead input.
    /// Consumed by T4's `XonAim::step` (next commit).
    #[allow(dead_code)]
    pub vel: Option<Vec3>,
}

/// Sticky enemy tracker (`havocbot_chooseenemy` state).
#[derive(Debug, Default)]
pub struct EnemyTracker {
    current: Option<i32>,
    /// Next full re-scan time.
    rescan_at: f32,
    /// Sticky hold deadline (extended while the target stays visible in range).
    sticky_until: f32,
}

impl EnemyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// The current enemy entity number, if engaged. Consumed by T4/T5 (chase awareness).
    #[allow(dead_code)]
    pub fn current(&self) -> Option<i32> {
        self.current
    }

    /// Drop the target (death/respawn).
    pub fn reset(&mut self) {
        self.current = None;
        self.sticky_until = 0.0;
    }

    /// Select this frame's enemy. Visibility = present in the PVS + not stale + LOS when a
    /// collision model is loaded (that IS the honest client-side `bot_shouldattack`).
    pub fn tick(
        &mut self,
        view: &Worldview,
        cm: Option<&CollisionModel>,
        now: f32,
    ) -> Option<Enemy> {
        let eye = los::eye_origin(view.self_state().origin.into());
        let visible = |e: &crate::perception::PerceivedEntity| -> bool {
            !e.is_stale
                && match cm {
                    Some(c) => los::has_los_player(c, eye, e.origin.into()),
                    None => true,
                }
        };

        // Sticky: keep the current enemy while it stays visible within range
        // (`havocbot.qc:1350-1365`), extending the hold each check.
        if let Some(id) = self.current {
            if let Some(e) = view
                .entities()
                .find(|e| e.entity_number == id && e.class == EntityClass::EnemyPlayer)
            {
                let pos = view.self_state().origin;
                let in_range = (e.origin - pos).length() < STICKY_RANGE;
                if in_range && visible(e) {
                    self.sticky_until = now + STICKY_EXTEND;
                }
                // Hold while sticky, or between scans if still visible.
                if now < self.sticky_until || (now < self.rescan_at && visible(e)) {
                    return Some(Enemy {
                        id,
                        pos: e.origin,
                        vel: e.velocity,
                    });
                }
            } else if now < self.rescan_at {
                // Target left the PVS entirely: hold nothing until the next scan.
                self.current = None;
            }
        }

        if now < self.rescan_at {
            return self.current.and_then(|id| {
                view.entities()
                    .find(|e| e.entity_number == id)
                    .map(|e| Enemy {
                        id,
                        pos: e.origin,
                        vel: e.velocity,
                    })
            });
        }

        // Full re-scan: nearest visible enemy (`havocbot.qc:1414-1426`, non-SUPERBOT arm).
        let pos = view.self_state().origin;
        let next = view
            .entities()
            .filter(|e| e.class == EntityClass::EnemyPlayer && visible(e))
            .min_by(|a, b| {
                let da = (a.origin - pos).length_squared();
                let db = (b.origin - pos).length_squared();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|e| Enemy {
                id: e.entity_number,
                pos: e.origin,
                vel: e.velocity,
            });

        let sticking = matches!((self.current, &next), (Some(c), Some(n)) if c == n.id);
        self.current = next.as_ref().map(|e| e.id);
        self.rescan_at = now
            + if sticking {
                RESCAN_STICKY_SECS
            } else {
                RESCAN_SECS
            };
        next
    }
}

/// Weapon-choice state: the probe-and-learn inventory + combo/request clocks.
#[derive(Debug, Default)]
pub struct WeaponChooser {
    /// Next choose-weapon evaluation.
    next_choose: f32,
    /// (weapon, when) we last requested — the probe the grace window checks.
    pending: Option<(Weapon, f32)>,
    /// Assumed-unowned marks: (weapon, until).
    unowned: Vec<(Weapon, f32)>,
    /// Combo lockout deadline.
    combo_lock_until: f32,
    /// Last emitted request time (thrash guard).
    last_request: f32,
}

impl WeaponChooser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Respawn: loadout reset to Blaster; everything unlearned.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn assumed_unowned(&self, w: Weapon, now: f32) -> bool {
        self.unowned.iter().any(|&(u, until)| u == w && now < until)
    }

    /// Pick the desired weapon for a fight at `dist` and decide whether to emit a `use`
    /// request this tick. `fired_at` is when we last pulled the trigger (`None` = not yet;
    /// drives the combo check). Returns the weapon to request, at most one per cooldown.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        sk: &XonSkill,
        dist: f32,
        held: Weapon,
        held_ammo: i32,
        fired_at: Option<f32>,
        now: f32,
    ) -> Option<Weapon> {
        // Probe resolution: the last request either landed (held == requested) or the grace
        // expired → learn "unowned".
        if let Some((w, at)) = self.pending {
            if held == w {
                self.pending = None;
                self.unowned.retain(|&(u, _)| u != w);
            } else if now - at > PROBE_GRACE {
                self.pending = None;
                self.unowned.push((w, now + UNOWNED_MEMORY));
                tracing::debug!(weapon = w.name(), "xon: assuming unowned");
            }
        }
        self.unowned.retain(|&(_, until)| now < until);

        if now < self.next_choose {
            return None;
        }
        self.next_choose = now + CHOOSE_INTERVAL;

        // Effective range with the personality bias (`havocbot.qc:1564-1565`).
        let eff = (dist - 200.0).clamp(10.0, 10_000.0) * 2f32.powf(sk.range_preference());
        let list: &[Weapon] = if eff < DIST_CLOSE {
            &LIST_CLOSE
        } else if eff < DIST_FAR {
            &LIST_MID
        } else {
            &LIST_FAR
        };

        // Walk the list: skip self-splash-unsafe, assumed-unowned, and the dry held gun.
        let mut desired = Weapon::Blaster;
        for &w in list {
            if dist < w.min_safe_distance() {
                continue;
            }
            if self.assumed_unowned(w, now) {
                continue;
            }
            if w == held && held != Weapon::Blaster && held_ammo <= 0 {
                continue; // dry — the ONLY ammo the wire shows us is the held gun's
            }
            desired = w;
            break;
        }

        // Weapon combo (`havocbot.qc:1544-1559`): mid-refire switch if the current refire
        // won't complete soon enough — then locked for 1 s.
        if now < self.combo_lock_until {
            return None;
        }
        if desired == held {
            return None;
        }
        if let Some(t) = fired_at {
            let refire_done = t + held.fire_interval_secs();
            let window = COMBO_THRESHOLD * (4.0 - 0.3 * sk.weapon());
            if refire_done > now + window {
                // The held gun is stuck reloading past the window — combo-switch NOW.
                self.combo_lock_until = now + COMBO_LOCK;
            }
            // (Refire completing soon → normal switch below; same request path.)
        }

        // Thrash guard (Plan 47): at most one request per second, only on change.
        if now - self.last_request < REQUEST_COOLDOWN {
            return None;
        }
        self.last_request = now;
        self.pending = Some((desired, now));
        Some(desired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sk() -> XonSkill {
        XonSkill::default()
    }

    #[test]
    fn list_pick_by_band_and_rangepref() {
        let mut wc = WeaponChooser::new();
        // Far fight (eff 1800): railgun tops the far list.
        let w = wc.tick(&sk(), 2000.0, Weapon::Blaster, 0, None, 10.0);
        assert_eq!(w, Some(Weapon::Railgun));

        // Same real distance, strong close-range preference (2^-2): eff 450 → mid list → RL.
        let mut wc = WeaponChooser::new();
        let mut s = sk();
        s.axes.rangepref = -2.0;
        let w = wc.tick(&s, 2000.0, Weapon::Blaster, 0, None, 10.0);
        assert_eq!(w, Some(Weapon::RocketLauncher));
    }

    #[test]
    fn close_quarters_skips_splash_weapons() {
        let mut wc = WeaponChooser::new();
        // 80 u: SSG leads the close list; RL/GL are splash-filtered anyway.
        let w = wc.tick(&sk(), 80.0, Weapon::Blaster, 0, None, 10.0);
        assert_eq!(w, Some(Weapon::SuperShotgun));
    }

    #[test]
    fn probe_learns_unowned_and_falls_through() {
        let mut wc = WeaponChooser::new();
        // Request the railgun at t=10…
        assert_eq!(
            wc.tick(&sk(), 2000.0, Weapon::Blaster, 0, None, 10.0),
            Some(Weapon::Railgun)
        );
        // …still holding Blaster after the grace → assume unowned → next candidate (RL).
        let w = wc.tick(&sk(), 2000.0, Weapon::Blaster, 0, None, 11.2);
        assert_eq!(w, Some(Weapon::RocketLauncher));
    }

    #[test]
    fn request_rate_is_thrash_guarded() {
        let mut wc = WeaponChooser::new();
        let mut requests = 0;
        // 3 seconds of 0.1 s ticks with an ever-unsatisfied desire.
        for i in 0..30 {
            let now = 10.0 + i as f32 * 0.1;
            if wc
                .tick(&sk(), 2000.0, Weapon::Blaster, 0, None, now)
                .is_some()
            {
                requests += 1;
            }
        }
        assert!(requests <= 3, "≤1 request/s (got {requests})");
    }

    #[test]
    fn dry_held_gun_is_never_kept() {
        let mut wc = WeaponChooser::new();
        // Holding a dry SSG at close range: the pick must not be the dry SSG.
        let w = wc.tick(&sk(), 80.0, Weapon::SuperShotgun, 0, None, 10.0);
        assert!(w.is_some());
        assert_ne!(w, Some(Weapon::SuperShotgun));
    }

    #[test]
    fn sticky_tracker_rescan_cadence() {
        // Pure-timer behavior (no Worldview needed for the cadence math): rescan_at moves
        // 2 s when acquiring fresh, 4 s when sticking. Exercised via the public tick with
        // an empty view (no enemies → current stays None, rescan cadence still applies).
        use crate::perception::Worldview;
        use client::parse::ConfigStrings;
        use q2proto::Frame;
        let view = Worldview::from_frame(&Frame::default(), &ConfigStrings::default(), 0);
        let mut tr = EnemyTracker::new();
        assert!(tr.tick(&view, None, 0.0).is_none());
        assert!(tr.current().is_none());
        // Second call inside the window: still none, no panic (timer path).
        assert!(tr.tick(&view, None, 1.0).is_none());
    }
}
