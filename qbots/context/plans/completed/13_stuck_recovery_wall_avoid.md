# Plan 13 — Reactive Stuck Recovery & Wall Avoidance

> **Status**: pending
> **Created**: 2026-06-15
> **Depends on**: Plan 12 (steering controller — recovery feeds `Steering`, not raw `mv`)
> **Goal**: When a bot stops making progress, it reacts like Eraser — fan-out trace for a
> clear heading, strafe/jump/back-off to unstick — instead of blindly reversing into the
> obstacle and replanning the same wedged route.

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Port Eraser's two recovery mechanisms to the external-client world, plus unify
the bot's split stuck detection:

1. **`find_best_direction`** — Eraser `botRoamFindBestDirection`: a 7-direction fan-out
   hull trace from the bot, scored by clear distance, penalized for ledges/liquid → a
   fallback heading when there's no usable nav node or the bot is wedged.
2. **Inline micro-recovery** — Eraser `bot_move` stuck branch: if moved <10% of expected,
   try straight `M_walkmove`; on wall, strafe ±110°; then jump; last resort random back-off.
   Mapped to `usercmd{forward, side, jump}` (we have no `M_walkmove`/direct velocity).
3. **Unified stuck detector** — one origin-ring-buffer deadband (Eraser: 4 u / 1 s cadence),
   replacing the divergent `nav.rs` (`<16u`/30t) and `main.rs` (`<1u`/50t) systems.
4. **Tightened give-up** — Eraser 2 s (if >128 u away) / 4 s hard cap, replacing the current
   8 s `GOAL_GIVEUP_TICKS`.

**Deliverables**: a `brain::recover::Recovery` controller returning a `RecoveryAction`
(jump / strafe-left / strafe-right / back-off / repath / none) that the steering pipeline
consumes as the highest-priority movement override; the recorder gains a `recovery` flag so
recovery events are countable in the logs.

**Estimated effort**: Medium (1 day) — the fan-out trace and micro-recovery are small,
well-specified (distilled §3, §9), and reuse `CollisionModel::trace`.

---

## Context

### Pre-Identified Bugs

1. **Blind reverse.** Stuck recovery in `bot_task` (`main.rs:681-687`):
   ```rust
   if nav.is_stuck() {
       mv.move_forward(-1.0);   // reverse — into whatever is behind us
       mv.jump();
       nav_driver.as_mut().unwrap().force_replan();
       nav_driver.as_mut().unwrap().reset_stuck();
   }
   ```
   Backing up view-relative pulls the bot *away from what it faces* — often into the wall
   behind it — and `force_replan` re-runs A* to the same goal, re-wedging on the same node.
   No strafe, no wall-scan.
2. **Two divergent stuck detectors.** `NavigationDriver` (`nav.rs:226-237`) flags stuck at
   `<16u` movement over 30 ticks (3 s) → `is_stuck()`; `bot_task` (`main.rs:738-753`) has a
   *separate* `<1u`/50-tick (5 s) warning that only logs. The 1u threshold is below normal
   standing jitter; the 16u/3s one is slow. Eraser uses a **4-unit deadband on a 1 s cadence**
   (distilled §3), jumping after one stuck cycle and suiciding (we can't) after 5 s.
3. **No fallback heading.** When `ClosestNodeToEnt == −1` (no node near), Eraser runs
   `botRoamFindBestDirection` (7-dir fan) to pick a clear yaw. qbots has no analog: if the
   nav graph has no node nearby, the bot has no steering at all.

### Why [approach]: reactive geometry, not just replanning

A* gives a *route*; it does not know the bot has drifted a foot off the edge or is grinding a
corner. Eraser's micro-recovery is reactive — it traces *right now, from the current pose*
and picks an open heading. That's exactly the gap when the nav graph (static, grid-sampled)
can't see the local snag. This plan layers **tactical** recovery *under* the steering
controller (Plan 12) and *over* plain A*.

### Key facts (distilled §3, §9)

- `botRoamFindBestDirection` (`bot_nav.c:96-176`): 7 directions at 45° (skips 4,5),
  `TRACE_DIST=256`, lifted by `STEPSIZE=24`; score `= fraction·256`; **halve score if a
  down-trace from the endpoint has `fraction>0.4`** (avoid long falls); skip liquid; early-out
  on a full-256 hit.
- Inline micro-recovery (`bot_move`, `bot_ai.c:2268-2378`): if `|move| < dist·0.1` →
  `M_walkmove` straight; if wall (`fraction<0.3`, near-vertical normal) → jump if head clear
  else abort goal; **strafe ±110°** (flip every 3 of 6 s); last resort `M_walkmove(yaw ±
  180·random, dist·0.5)`.
- Give-up watchdog (`bot_ai.c:848-887`): abandon goal if `(last_reach < now−2 AND dist>128)`
  OR `last_reach < now−4`; blacklist movetarget +3 s, enemy +1 s, goalentity +0.5 s.
- **Suicide-by-respawn is unavailable over UDP** (distilled §3) — drop it; lean on
  jump/strafe/retrail. `M_walkmove`/`velocity=` → `usercmd`.

---

## Step-by-Step Tasks

### T1: Unified stuck detector

**File**: `crates/brain/src/recover.rs` (new), export from `lib.rs`; remove the duplicate in
`main.rs`.

**What to do**: A `StuckDetector` with a small ring buffer of recent origins (e.g. 16 slots)
and a 1 s cadence check. On each sample:
```rust
const DEADBAND: f32 = 4.0;          // Eraser 4-unit
const SAMPLE_EVERY_SECS: f32 = 1.0; // Eraser cadence
const JUMP_AFTER_SECS: f32 = 1.0;   // first stuck cycle → jump
const HARD_REPATH_SECS: f32 = 5.0;  // sustained → force replan (was suicide)
```
Track `stuck_secs` cumulative. Returns `StuckLevel { None, Mild(jump), Hard(repath) }`.
Replace `NavigationDriver`'s `stuck_ticks`/`is_stuck` (`nav.rs:226-266`) and the `main.rs`
`stuck_frames`/`STUCK_WARNING_FRAMES` block (`main.rs:738-753`) with calls into this one type.

**Tests** (`brain/tests/recover.rs`): moving >4u/s → never stuck; stalled → `Mild` after 1 s,
`Hard` after 5 s; resumes moving resets.

**Verify**: `cargo test -p brain`; `cargo clippy -p brain -- -D warnings`.

### T2: `find_best_direction` fan-out

**File**: `crates/brain/src/recover.rs`.

**What to do**: Port `botRoamFindBestDirection`:
```rust
pub fn find_best_direction(cm, origin, view_yaw) -> Option<(f32 /*yaw*/, f32 /*score*/)> {
    for i in [0,1,2,3,6,7] {                       // 6 dirs around view_yaw at 45° (skip rear pair)
        let yaw = view_yaw + (i as f32 - 3.0) * 45.0;
        let dir = forward_from_yaw(yaw);
        let lifted = origin + (0,0, STEPSIZE);     // lift like Eraser
        let t = cm.trace(&lifted, &(lifted + dir*TRACE_DIST), &HULL_MINS, &HULL_MAXS, MASK_SOLID);
        if t.startsolid { continue; }
        let mut score = t.fraction * TRACE_DIST;   // 0..256
        // down-trace from endpoint: penalize long falls / reward solid floor
        let down = cm.trace(&t.endpos, &(t.endpos - (0,0,256)), &[0;3], &[0;3], MASK_SOLID|MASK_WATER);
        if down.fraction > 0.4 { score *= 0.5; }    // ledge risk
        if liquid_at(&down) { continue; }           // skip lava/slime/water floor
        track best;
    }
    best
}
```
Constants `TRACE_DIST=256`, `STEPSIZE=24` (Eraser; verify Q2 server `STEPSIZE` — distilled
notes flag 18 vs 24). Use the player hull for the forward probe (matches what the bot can
actually fit through). Early-out on `score == TRACE_DIST`.

**Tests**: open field → best yaw ≈ forward; wall ahead, gap to the right → picks the right;
ledge ahead → penalized; all blocked → `None` (caller falls back to jump/replan).

**Verify**: `cargo test -p brain`.

### T3: Inline micro-recovery → `RecoveryAction`

**File**: `crates/brain/src/recover.rs`.

**What to do**: Combine T1 + T2 into a `Recovery::evaluate(view, nav) -> RecoveryAction`:
```rust
pub enum RecoveryAction {
    None,
    Jump,                 // first-cycle stuck
    Strafe { dir: i8 },   // ±110° mapped to side; flip every 3 of 6 s
    BackOffThenRepath,    // sustained — back off a step + force replan + blacklist goal briefly
    UseHeading(f32),      // no node nearby → steer at find_best_direction yaw
}
```
Map to `MovementIntent` in the steering pipeline (Plan 12): `Jump`→`mv.jump()`; `Strafe`→
`mv.move_side(±1)` with `forward≈0`; `BackOffThenRepath`→`mv.move_forward(-0.5)` + jump +
`nav.force_replan()` + a transient goal blacklist (Eraser `ignore_time` on the goalentity,
~0.5 s — add a small `blacklist` set/timer on `NavigationDriver`); `UseHeading(yaw)`→feed as
the ideal yaw + forward when `pursue_target` is `None`.

Priority: dodge (Plan 07) > **recovery (this)** > engage/pursue (Plan 12). The steering
controller checks `Recovery` first.

**Tests**: stalled + wall ahead + gap right → `Strafe{+}` or `Jump`; sustained stall →
`BackOffThenRepath`; no node + open forward → `UseHeading`.

**Verify**: `cargo test -p brain`; `cargo clippy -p brain -- -D warnings`.

### T4: Wire into the steering pipeline + recorder flag

**File**: `crates/qbots/src/main.rs` (`bot_task`), `crates/brain/src/recorder.rs`.

**What to do**:
1. Replace the `main.rs:681-687` blind-reverse block with `let action = recovery.evaluate(..)`
   consumed by `Steering` (highest priority). Remove the `main.rs:738-753` stuck-warning block
   (T1 owns it now).
2. Add `NavigationDriver` goal-blacklist (transient `ignore` on the just-abandoned goal node
   so a replan doesn't immediately re-pick it — Eraser +0.5 s).
3. Recorder: `FrameRecord.recovery: Option<&'static str>` (`"jump"/"strafeL"/"strafeR"/"backoff"/"heading"`)
   so the SUMMARY counts recovery events. After this plan, sustained-stuck sequences should be
   short (a jump/strafe, then progress) rather than multi-second grinds.

**Verify**: `cargo build -p qbots`, zero warnings; `cargo clippy -p qbots -- -D warnings`.
Plan-10 scenarios: `hindered` runs shorten; no bot stalls >5 s without a recovery action firing.

### T5: Live confirmation + pitfall note

**File**: `context/pitfalls.md` (append).

**What to do**: Run `spawn-to-spawn` on a map with tight corners (q2dm1 has several). Confirm
bots no longer freeze-against-wall for 8 s. Record the "two divergent stuck detectors +
blind reverse" pitfall.

**Verify**: before/after `hindered`/recovery counts in the tracker.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/recover.rs` | NEW — `StuckDetector`, `find_best_direction`, `Recovery`/`RecoveryAction` | P0 |
| `crates/brain/src/lib.rs` | export `recover` | P0 |
| `crates/brain/src/nav.rs` | remove `stuck_ticks`/`is_stuck`/`reset_stuck` (T1 owns it); add transient goal blacklist | P0 |
| `crates/qbots/src/main.rs` | consume `Recovery` in steering; remove blind-reverse + stuck-warning blocks | P0 |
| `crates/brain/src/recorder.rs` | `recovery` flag | P1 |
| `crates/qbots/src/supervisor.rs` / `MapNav` | confirm `Arc<CollisionModel>` reachable (also Plan 11 T2b) | P1 |
| `context/pitfalls.md` | stuck-detector/reverse pitfall | P2 |

---

## Open Questions / Risks

1. **`STEPSIZE` 18 vs 24**: distilled flags the mismatch. The lift in `find_best_direction`
   and any step-climb assumption must match the *server's* pmove `STEPSIZE`. Make it a
   constant to tune; instrument the recorder to log failed step-climbs if bots stall at stairs.
2. **Strafe mapping**: Eraser's ±110° yaw strafe maps to our view-relative `side` only if the
   bot faces the original heading. In the steering controller (Plan 12) the view may be on an
   enemy; map the recovery strafe as a **world** lateral vector decomposed via
   `move_from_world_dir` (Plan 12 T2) so it's correct regardless of view yaw.
3. **Recovery vs. combat**: a bot mid-duel that stalls shouldn't necessarily abandon the
   fight to strafe a wall. Gate `BackOffThenRepath` (goal abandonment) on `!engaging` or on
   `engaging && no_LOS`; allow `Jump`/`Strafe` always (they don't drop the target).
4. **Blacklist thrash**: a too-long goal blacklist can make the bot wander. Keep it ≤0.5 s
   (Eraser) and only on the specific abandoned node, not the whole goal class.
5. **`find_best_direction` cost**: 6 hull traces per stuck tick — fine (stuck ticks are rare).
   Do **not** run it every tick, only when `StuckLevel != None` or when `pursue_target` is `None`.

---

## Verification Checklist

- [ ] T1: unified `StuckDetector` — Mild@1s / Hard@5s unit tests; old duplicate detectors removed.
- [ ] T2: `find_best_direction` open/wall/ledge/blocked unit tests green.
- [ ] T3: `Recovery::evaluate` action-selection unit tests green.
- [ ] T3: `cargo clippy -p brain -- -D warnings` clean.
- [ ] T4: `cargo build -p qbots`, zero warnings; `cargo clippy -p qbots -- -D warnings` clean.
- [ ] T4: no bot stalls >5 s without a recovery action in the log; `hindered` runs shorten vs baseline.
- [ ] T4: goal blacklist prevents immediate re-wedge (a stuck corner doesn't loop the same node).
- [ ] T5: live `spawn-to-spawn` on a tight-corner map shows bots unstick within ~1 s.
- [ ] T5: `context/pitfalls.md` updated.
