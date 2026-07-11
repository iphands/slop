# Plan 51 — zb2 Combat Wall-Stall: Instrument, Prove, Fix

> **Status**: in-progress
> **Created**: 2026-07-11
> **Depends on**: Plan 48 (zb2 wall-running fixes), Plan 49 (pain response), Plan 50 (EVT instrumentation discipline)
> **Goal**: Prove with live-soak instrumentation why zb2 runs face-first into walls and stalls during combat, then fix the verified root cause and show the episode count drop.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Add an always-on, brain-agnostic wall-press/stall episode detector (`EVT wall_press`) to the live fleet tick, soak-baseline it, prove the zb2 combat-stall root cause from the logs, fix it, and re-soak.

**Deliverables**:
1. `brain::stall::StallMonitor` — pure, unit-tested episode detector (intent-vs-motion mismatch), emitting one `EVT wall_press` line per episode with duration, mean speed, attack share, damage taken, wall-blocked share, and died-in-episode.
2. A hypothesis probe in `zb2` (`EVT zb2_combat_recovery_overwrite`) — logs when recovery emitted an action that the run-and-gun movement block then overwrote (no behavior change).
3. Baseline soak numbers (305 s, `competition --count 3 --brains main,q3,zb2 --navmodes astar`, q2dm3) recorded in the tracker: episodes/brain, combat share, damage eaten while stalled.
4. The proven fix + re-soak comparison in `context/brain_notes.md`.

**Estimated effort**: Medium (1 day)

## Context

Live report (2026-07-11, after Plans 48–50 shipped): **zb2 still consistently runs face-forward into walls and stalls during combat** — a sitting duck. Reported on both `--navmode astar` and `fallback`; note zb2 ignores `--navmode` entirely (always A*-graph-routed, see `zb2.rs` module docs), so mode-independence is expected and points *inside* zb2 or the shared combat/recovery plumbing.

### Prior art
- Plan 48 Z1–Z3 fixed *out-of-combat* zb2 wall grinding (shortcut walkability, no-route freeze, stuck-replan goal blocking). The live report says the residual stall is **in combat**.
- Plan 50's core lesson (pitfalls: "instrument first"): the lava fix only landed after `EVT lava_escape` carried position+velocity. Same discipline here — no fix until an EVT stream proves the mechanism.

### Pre-Identified Suspects (to prove or kill with data — NOT assumed)
1. **Recovery output is clobbered while firing** (`zb2.rs` step 5 → step 6): when `combat_dec.should_fire`, the run-and-gun block re-sets `mv.move_forward/move_side` from `world_dir` AFTER `RecoveryAction::Strafe`/`UseHeading` wrote theirs — so while an enemy is visible, stuck recovery does nothing. Compounding: `recover.rs::evaluate(engaging=true)` never returns `BackOffThenRepath`, so `route.dirty` is never set in combat — no replan escape either. Prediction if true: `EVT zb2_combat_recovery_overwrite` fires continuously inside `EVT wall_press` episodes that have high attack share.
2. **Route `world_dir` points into a wall while the view is enemy-locked**: the committed polyline's next node can sit behind a corner; `move_from_world_dir(world_dir, aim_yaw, false)` (raw mode) walks the bot straight into the corner at full speed with no face-then-go throttle. Prediction if true: wall-press episodes cluster at corners with `wall_pct` high and the wish direction pointing at the current waypoint.
3. **Stale/no-LOS chase**: `should_fire=false` on stale targets, so the stale path shouldn't hit suspect 1 — but verify episodes aren't dominated by `attacking=false` combat pursuit (would point elsewhere, e.g. steering or arrive-scale).

## Step-by-Step Tasks

### T1: `StallMonitor` + fleet wiring + zb2 hypothesis probe

**Files**: `crates/brain/src/stall.rs` (new), `crates/brain/src/lib.rs`, `crates/qbots/src/main.rs`, `crates/brain/src/brains/zb2.rs`

**What to do**:
- New module `brain::stall` with `StallSample` (pos, horizontal speed, intent magnitude, attacking, wall_blocked, damage this tick, alive, dt) and `StallMonitor::tick(sample) -> Option<StallEpisode>`.
  - Hindered tick = `intent_mag > 0.5` AND `speed_h < 40 u/s` (walk speed is 320; the stuck detector's deadband is 16 u/s — 40 catches hard grinding without flagging normal accel/turn frames).
  - Open an episode after 5 consecutive hindered ticks (~0.5 s); close after 3 consecutive free ticks or on death (`died=true`).
  - Episode carries: start pos, seconds, mean speed, attack-tick share, wall-blocked share, damage absorbed, died flag.
  - Unit tests: opens only after threshold, closes on recovery, death closes with `died`, accumulators correct.
- Fleet tick (`main.rs` bot task): one `StallMonitor` per bot, fed after `brain.tick()` from `out.intent` + playerstate velocity; `wall_blocked` = hull trace 28 u along the world-space wish dir (from `intent.yaw` via `steer::view_forward/view_right`), only computed on hindered ticks. Emit `tracing::info!("EVT wall_press", …)` on close.
- zb2 probe: remember the `RecoveryAction` label from step 5; if step 6's `should_fire && !traversing` branch runs while that label is `Some`, `tracing::info!(action, wp_dist, "EVT zb2_combat_recovery_overwrite")`. No behavior change.

### T2: Baseline instrumented soak + analysis

**What to do**: 305 s soak: `cargo run -q -p qbots --release -- competition --count 3 --brains main,q3,zb2 --navmodes astar > logs/p51_baseline.log 2>&1` (server: config default, q2dm3 — verify with `status` first). Then grep/aggregate: `EVT wall_press` count + total seconds + damage per brain group; share with `atk>0`; `EVT zb2_combat_recovery_overwrite` correlation. Record the table in the tracker. This is the go/no-go gate: the fix targets whatever mechanism the data indicts.

### T3: Fix the proven root cause

**Files**: expected `crates/brain/src/brains/zb2.rs` (and possibly `recover.rs`)

**What to do**: Determined by T2's verdict. If suspect 1: apply recovery output *after* (or into) the run-and-gun decomposition — e.g. when recovery is active, let the strafe/heading own the legs (re-decomposed against `aim_yaw` so the view stays on the enemy) instead of the blocked `world_dir`; consider letting sustained combat stalls still set `route.dirty`. Keep the fight-on-the-run character. Unit test the chosen mechanism.

### T4: Re-soak, compare, document, close

**What to do**: Same soak as T2 → `logs/p51_postfix.log`; compare episode count/duration/damage for zb2 (target: combat-stall episodes cut by well over half, no regression for main/q3). Append a dated section to `context/brain_notes.md`; add a pitfall if the mechanism was non-obvious. `git mv` plan+tracker to `completed/`, mark SERIES.md done.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/stall.rs` | New episode detector + tests | P0 |
| `crates/qbots/src/main.rs` | Feed monitor in fleet tick, emit EVT | P0 |
| `crates/brain/src/brains/zb2.rs` | Hypothesis probe (T1), then the fix (T3) | P0 |
| `crates/brain/src/recover.rs` | Possible engage-gate change (T3, data-dependent) | P1 |
| `context/brain_notes.md` | Findings + numbers | P1 |

## Open Questions / Risks

1. **Single-run variance** (Plan 47 noise floor): episode *counts* are the metric, not K/D — counts were stable across Plan 50's 8 soaks; still, treat <2× changes as noise and re-run once if borderline.
2. **False positives from lava swims/ladders/lifts**: low speed with high intent is legitimate there. Mitigation: episodes carry position (correlate with known lava/lift spots); if noisy, gate on `wall_pct` for the analysis rather than raw count.
3. **A human player on the server** during soaks changes combat pressure between runs. Acceptable — compare zb2 against main/q3 *within* the same run, not only across runs.
4. **The probe encodes suspect 1** — guard against confirmation bias: T2 analysis must also check suspect 2 (corner clustering / wall_pct with no recovery-overwrite events) before choosing the T3 fix.

## Verification Checklist

- [ ] T1: `cargo test -p brain` green incl. new `stall` tests; `just all` clean; live `connect-one` shows `EVT wall_press` lines when a bot is shoved into a corner (or none if never hindered). **Committed.**
- [ ] T2: Baseline table in tracker (episodes / secs / dmg / atk-share per brain; overwrite-event correlation). **Committed.**
- [ ] T3: Root-cause fix implemented + unit-tested; `just all` clean. **Committed.**
- [ ] T4: Post-fix soak shows zb2 combat-stall episodes cut >50% with main/q3 flat; brain_notes updated; plan moved to `completed/`. **Committed.**
