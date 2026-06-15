# Plan 11 — Honest Line-of-Sight Perception

> **Status**: pending
> **Created**: 2026-06-15
> **Depends on**: Plan 10 (recorder confirms phantom-chase symptom in baseline logs)
> **Goal**: Bots only react to (target, fire at, chase, navigate toward) enemies they can
> actually see — gated by a BSP trace, not a bare FOV cone.

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Add a reusable **line-of-sight** check (`world` trace from eye-height to a target
point, player hull, `MASK_SOLID`), wire it as a visibility gate into (a) enemy target
selection, (b) FSM's Roam→Engage transition, (c) the fire decision, and (d) nav-to-enemy
goal validity. Enemies behind walls become *unseen* — not chased through geometry.

**Deliverables**:
1. `brain::los::has_los(cm, from_eye, to_target) -> bool` (thin wrapper over
   `CollisionModel::trace`; `fraction >= 1.0` ⇒ clear).
2. A `Visibility`/`LosChecker` handle threaded into the places that currently call
   `view.nearest_enemy(90.0)`, so they take a **trace-confirmed** nearest enemy instead.
3. Target-acquire/loss hysteresis: lose sight → keep the target as *last-known* for
   `SIGHT_GRACE = 0.2 s` (Eraser `last_enemy_sight`), then drop to Hunt (last-known pos),
   matching Eraser §2 sighting semantics.
4. Plan-10 recorder gains a `phantom_target` flag (firing/chasing with no LOS) so we can
   prove the count drops to ~0 after this plan.

**Estimated effort**: Small–Medium (half day) — the trace exists; this is wiring + hysteresis.

---

## Context

### Pre-Identified Bug

`view.nearest_enemy(fov)` (`crates/brain/src/perception.rs:266-281`) selects the nearest
enemy whose direction is within a FOV cone — **no geometry test**:

```rust
self.enemies()
    .filter(|e| {
        let direction = (e.origin - origin).normalize();
        forward.dot(direction) > fov_radians.cos()   // FOV only — walls ignored
    })
    .min_by(...)
```

It is called from two load-bearing sites, both of which then act on a possibly-walled enemy:

- **`CombatDriver::select_target_entity`** (`combat.rs:198`) → sets `current_target` →
  `combat.evaluate` aims at and fires at it (`combat.rs:162`), and `main.rs:544-568`
  overrides the FSM to Engage + sets `nav_goal = NavGoal::Entity(target.origin)`.
- **`FSM::transition`** (`fsm.rs:87`) → flips Roam→Engage on a cone-only "seeing enemy".

Net effect: a bot "sees" an enemy through a wall, walks straight at the wall trying to
reach `target.origin`, fires into the wall, and never reconsiders until the 8 s goal give-up
(`nav.rs GOAL_GIVEUP_TICKS`) fires. **This is the single biggest cause of "bumping into
walls" and "not chasing players properly".**

### Why [approach]: trace gate, not a new perception struct

`Worldview::from_frame` is pure-data (no `CollisionModel` handle), and rightly so — it's
built every tick and tested without geometry. Rather than push a `CollisionModel` into
`from_frame`, add a **separate LOS query** the brain calls at decision time. This keeps the
data/decision boundary clean and makes LOS unit-testable against a tiny synthetic
`CollisionModel` (1 wall).

Eraser's model (distilled §2): `visible()` is a point LOS trace; `CanSee` adds a distance/
FOV gate; `last_enemy_sight` is refreshed only while `visible && inPVS`, with a 0.2 s grace.
We already get PVS for free (the server only sends PVS entities), so we only add the trace.

### Key facts

- `CollisionModel::trace(start, end, mins, maxs, mask) -> Trace { fraction, endsolid, endpos, ... }`
  (`crates/world/src/collision.rs`). `fraction >= 1.0` ⇒ unobstructed.
- Eye height: standing `viewheight ≈ 22` above origin (origin is ~24 above floor;
  `pmove.c` `pm_viewheight`). Trace from `origin + (0,0,22-8)=+14`? Match Eraser's
  `start = origin + forward*8; start[2] += viewheight−8` (distilled §5). For a pure LOS
  test a single eye→chest trace suffices; optionally also eye→feet for partial cover.
- Player hull `HULL_MINS/HULL_MAXS` (`world::navgraph`) — but for LOS a **point/zero-size**
  trace is more correct (we care whether the *line* is clear, not whether a player box fits).
  Use `mins=maxs=[0;3]` for LOS; reserve hull traces for movement (Plan 12/13).
- The PVS already pre-filters: if an entity isn't in our frame at all, it's not `enemies()`,
  so LOS is only run on PVS-visible candidates. Cheap.

---

## Step-by-Step Tasks

### T1: LOS helper

**File**: `crates/brain/src/los.rs` (new), export from `lib.rs`.

**What to do**:
```rust
pub fn has_los(cm: &CollisionModel, eye: [f32;3], target: [f32;3]) -> bool {
    let t = cm.trace(&eye, &target, &[0.0;3], &[0.0;3], world::MASK_SOLID);
    t.fraction >= 1.0 && !t.startsolid
}
/// Eraser-style two-point check: clear if eye→chest OR eye→feet is open
/// (lets us see enemies partially behind low cover).
pub fn has_los_player(cm: &CollisionModel, eye: [f32;3], enemy_origin: [f32;3]) -> bool {
    has_los(cm, eye, chest) || has_los(cm, eye, feet)
}
```
Constants: `EYE_Z = 22.0` (standing viewheight over origin), enemy chest ≈ `origin + 12`,
feet ≈ `origin - 20`. (Verify against `ps.viewoffset` live if available; the recorder can
dump it.) Expose `eye_origin(self_origin)` for callers.

**Tests** (`brain/tests/los.rs`): build a 1-wall `CollisionModel` (or a NavGraph-style
fixture); confirm `has_los` true on a clear line, false through the wall; `startsolid`
returns false.

**Verify**: `cargo test -p brain`; `cargo clippy -p brain -- -D warnings`.

### T2: Trace-confirmed nearest enemy

**File**: `crates/brain/src/perception.rs` (+ thread a `&CollisionModel`/`&LosChecker` to callers).

**What to do**: Add `Worldview::nearest_visible_enemy(&self, cm, fov)` — same as
`nearest_enemy` but additionally `has_los_player(cm, self_eye, e.origin)`. Keep the old
`nearest_enemy` for non-LOS uses (e.g. Hunt last-known). Because `Worldview` is pure data,
the `cm` is passed by the caller (combat/FSM already have access to the nav/world via the
task, which owns `Arc<CollisionModel>` — confirm in `main.rs`/`supervisor.rs`; if not held,
add it next to `Arc<NavGraph>` in `MapNav`).

Wire the callers:
- `CombatDriver::evaluate` needs `cm`: change its signature to take `&CollisionModel` (or a
  `&LosChecker` wrapping it) and call `nearest_visible_enemy` in `select_target_entity`.
- `FSM::transition` (`fsm.rs:87`): same — take `cm`, use `nearest_visible_enemy`.

**Before** (`combat.rs:188-202`):
```rust
fn select_target_entity(&mut self, view: &Worldview) -> Option<i32> {
    ...
    if let Some(t) = view.nearest_enemy(90.0) {            // FOV only — through walls
        self.current_target = Some(t.entity_number);
        ...
```
**After**:
```rust
fn select_target_entity(&mut self, view: &Worldview, los: &LosChecker) -> Option<i32> {
    ...
    if let Some(t) = view.nearest_visible_enemy(los.cm(), 90.0) {  // trace-gated
        self.current_target = Some(t.entity_number);
        ...
```

**Verify**: `cargo test -p brain`; `cargo clippy -p brain -- -D warnings`. Add a test where
the nearest enemy is behind a wall and a farther enemy is in the open → the open one is chosen.

### T3: Sight hysteresis (acquire/loss with 0.2 s grace)

**File**: `crates/brain/src/combat.rs`.

**What to do**: Track `last_los_frame` for the current target. While LOS holds, refresh it;
when LOS drops, keep the target (and allow firing) for `SIGHT_GRACE_FRAMES = 2` (~0.2 s at
10 Hz) — Eraser's `last_enemy_sight > now−0.2` gate (distilled §5 fire gate). After grace,
clear the target → FSM falls to Hunt with the last-known origin (already implemented at
`fsm.rs:122-131`). This prevents flicker when an enemy strafes behind a thin pillar for a
frame or two.

**Verify**: unit test — target visible → not → not: fire allowed for 2 frames, then dropped.

### T4: Nav-to-enemy validity + phantom-target recorder flag

**File**: `crates/qbots/src/main.rs` (the combat-target→nav override at `main.rs:544-568`),
`crates/brain/src/recorder.rs`.

**What to do**:
1. In `main.rs`, only set `nav_goal = NavGoal::Entity(target.origin)` when LOS to that
   target currently holds; otherwise let the FSM run (Roam/Hunt with last-known pos). This
   stops the "walk into the wall toward a walled enemy" behavior at its source.
2. Add `FrameRecord.phantom_target: bool` = true when `combat_dec.should_fire` or
   `target_entity.is_some()` **but** `has_los` is false. (Requires the recorder to also see
   `cm` + the target origin — pass them in via the existing `set_intent_forward`-style hooks.)
   The baseline log (Plan 10) should show many phantom frames; after Plan 11, ~0.

**Verify**: re-run Plan-10 scenarios; `phantom_target` frames in the SUMMARY should drop to
~0. No new warnings.

### T5: Live confirmation + pitfall note

**File**: `context/pitfalls.md` (append), `context/distilled.md` (append a 2-line LOS note).

**What to do**: Run `spawn-to-weapon` on a busy server and confirm the bot no longer
grinds into walls aiming at walled enemies (bumps drop in the log). Record the
"FOV-without-trace targets through walls" pitfall in `pitfalls.md` (it wasted effort
elsewhere in the Q2 ecosystem and is exactly the class of bug the distilled `delta_angles`
pitfall warns about — silent direction errors).

**Verify**: before/after `bumps`/`phantom_target` numbers in the tracker; pitfalls.md updated.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/los.rs` | NEW — `has_los`, `has_los_player`, `eye_origin` | P0 |
| `crates/brain/src/lib.rs` | export `los` | P0 |
| `crates/brain/src/perception.rs` | `nearest_visible_enemy(cm, fov)` | P0 |
| `crates/brain/src/combat.rs` | LOS-gated `select_target_entity`; `last_los_frame` hysteresis; `evaluate` takes `cm`/`LosChecker` | P0 |
| `crates/brain/src/fsm.rs` | `transition` uses `nearest_visible_enemy` | P0 |
| `crates/qbots/src/main.rs` | nav-to-enemy only on LOS; thread `cm` to combat/fsm; recorder hooks | P0 |
| `crates/qbots/src/supervisor.rs` / `MapNav` | ensure `Arc<CollisionModel>` is available to the tick (alongside `Arc<NavGraph>`) | P1 |
| `crates/brain/src/recorder.rs` | `phantom_target` flag | P1 |
| `context/pitfalls.md`, `context/distilled.md` | LOS pitfall + note | P2 |

---

## Open Questions / Risks

1. **`CollisionModel` availability in the tick**: `MapNav` holds `Arc<NavGraph>`; confirm it
   (or a sibling) holds the `CollisionModel` (it must — the graph is *built from* it at
   `supervisor::build_map_nav`). If only the graph is cached, cache the `cm` too (it's
   already in memory; just retain the `Arc`). Low risk.
2. **Trace cost per tick**: one trace per visible enemy per frame at 10 Hz × ≤8 enemies is
   negligible. But the fleet runs 8 bots × this; keep the LOS pass to *candidate* enemies
   (already PVS + FOV filtered), not all entities.
3. **Eye-height accuracy**: if `viewheight` is wrong, LOS through waist-high cover mis-fires.
   Mitigation: the two-point (chest+feet) check tolerates a few units of error; optionally
   read `ps.viewoffset` live and log it via the recorder in T4.
4. **Stale-enemy fallback**: `select_target_entity` currently falls back to the nearest
   *stale* (last-known) enemy (`combat.rs:204-218`). That's a Hunt behavior, not an Engage —
   keep it but **do not fire** on a stale target (no LOS by definition). Ensure `should_fire`
   is false while LOS is absent (the hysteresis in T3 handles the transient; stale is permanent).
5. **Combat signature change ripples**: `evaluate` is called from `main.rs:541`; threading
   `cm` touches one call site. Keep the change minimal (a `&LosChecker` borrowed ref, not ownership).

---

## Verification Checklist

- [ ] T1: `cargo test -p brain` — `has_los` clear/blocked/startsolid cases green.
- [ ] T2: `nearest_visible_enemy` picks the open enemy over a nearer walled one (unit test).
- [ ] T2: `cargo clippy -p brain -- -D warnings` clean.
- [ ] T3: hysteresis unit test — fire allowed for 2 frames after LOS loss, then dropped.
- [ ] T4: `cargo build -p qbots`, zero warnings; `cargo clippy -p qbots -- -D warnings` clean.
- [ ] T4: nav-to-enemy only set when LOS holds (code review / a targeted unit test).
- [ ] T4: `phantom_target` SUMMARY count ≈ 0 after the change (vs >0 baseline).
- [ ] T5: `spawn-to-weapon` live run shows fewer `bumps` than the Plan-10 baseline.
- [ ] T5: `context/pitfalls.md` + `context/distilled.md` updated with the LOS finding.
