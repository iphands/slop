# Plan 26 — `runtester` Scenario Brain (+ 6-navmode regression gate)

> **Status**: pending
> **Created**: 2026-06-18
> **Depends on**: Plan 25 (multibrain selection + `--navmode` rename), Plan 24 (`MainBrain` relocated)
> **Goal**: Promote `scenario.rs`'s inline non-combat pathfinding loop into a first-class
> `RuntesterBrain` plugin (`BrainKind::Runtester`), make `spawn-to-*` drive a selectable
> `Box<dyn Brain>` (default `runtester`, `main` available for A/B), delete the inline duplication,
> and prove all **6 navmodes** still match `context/mode_perf.md`.
> **Agent**: sub-agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: `scenario.rs`'s tick loop is already a careful, single-minded, non-combat
waypoint-seeker — *not* "MainBrain with combat off" (it uses the corner-safe `pursue_target_safe`
and a richer 7-ray backoff/escape that MainBrain lacks). Lift that body **verbatim** into a
`RuntesterBrain` implementing `trait Brain`, register it as `BrainKind::Runtester`, migrate
`scenario.rs` onto a `Box<dyn Brain>` selected by `--brain` (default `runtester`), and delete the
duplicated decision block. Guard the refactor with CI determinism tests **and** a live
6-navmode acceptance sweep that must score ≥ the `mode_perf.md` baseline.

**Deliverables**:
1. `BrainContext.goal_override: Option<NavGoal>` (per-tick goal injection); `BrainConfig.goal_override` dropped.
2. `brain::brains::runtester::RuntesterBrain` (verbatim lift) + `BrainKind::Runtester` + factory/tag.
3. CI determinism unit tests pinning `RuntesterBrain::tick` over a synthetic CM + stub `Navigator`.
4. `scenario.rs` runs a `Box<dyn Brain>` (default `runtester`); inline block deleted — **closes Plan 22 T4 + retires Plan 15 duplication**.
5. (optional) `crates/tools` `mode-perf-report` log aggregator.
6. Live 6-navmode sweep ≥ `mode_perf.md` baseline; dated comparison appended to `mode_perf.md`; Plan 26 section in `context/brain_notes.md`.

**Estimated effort**: Medium (1 day + live-sweep iteration).

---

## Context

### Why `runtester` is a real second brain, not a config flag

`crates/qbots/src/scenario.rs:358–467` (the per-tick decision/steer/recovery body) differs from
`MainBrain::tick` in ways that materially help pure pathfinding:

| | `scenario.rs` (today) | `MainBrain::tick` |
|---|---|---|
| Look-ahead | `pursue_target_safe` (hull + floor validated) | unsafe `pursue_target` |
| Stuck escape | `backoff_ticks` + `escape_yaw` + `find_best_direction` (7-ray fan) | terse `BackOffThenRepath` (back 0.5 + replan) |
| Narrow geometry | `speed_scale` slowdown | — |
| Posture | always face-then-go, `Steering::new(3.0)` | circle-strafe / back-up on engage |

So the scenario loop is *already* a distinct brain; the plugin system (Plans 23–25) gives it a
home. Making it `BrainKind::Runtester` earns the abstraction (two genuinely different brains),
removes the Plan 15 duplication, and yields a reusable pure-pathfinder (fleets of pathfinders; a
per-`--navmode` A/B harness). **User decision (2026-06-18): keep both** — `MainBrain` retains its
`combat_enabled` knob so `spawn-to-spawn --brain main` can A/B the live brain's pathing.

### Why a verbatim lift (and what stays in the harness)

`RuntesterBrain::tick` is the **same code relocated**, so there is no behavior to "re-derive" —
parity is structural. The harness (`run_scenario`) keeps what it owns: connection, lazy goal
resolution (`resolve_goal` / `farthest_reachable_spawn`), the `MovementRecorder`,
reach-detection, `finalize`/exit codes. The brain owns only the per-tick decision + the
steering/recovery state it needs as fields.

### Why goal injection moves to `BrainContext`

The scenario goal is resolved *lazily* (farthest reachable spawn picked on the first active
frame), so a static `BrainConfig.goal_override` (Plan 23) can't carry it. Add
`goal_override: Option<NavGoal>` to `BrainContext` (per-tick); brains drive it when `Some`. Its
only consumer was the scenario path, so the static `BrainConfig.goal_override` is dropped;
`BrainConfig.combat_enabled` stays (the `--brain main` A/B path uses it).

### The regression contract — all 6 navmodes ≥ `mode_perf.md`

`RuntesterBrain` is **navmode-agnostic**: it drives `astar`, `navmesh`, and the four `hybrid-*`
backends through the same `Navigator` trait. A faithful lift must therefore preserve all six.
Baseline to match (q2dm1, 16 bots, 180 s cap, combat off — see `context/mode_perf.md`):

| navmode | s2s reached | s2w(RL) reached |
|---|:--:|:--:|
| astar | 16/16 | 12/16 |
| navmesh | 5/16 | 15/16 |
| hybrid-fallback | 14/16 | 12/16 |
| hybrid-race | 15/16 | 16/16 |
| hybrid-hier | 11/16 | 1/16 |
| hybrid-segment | 13/16 | 4/16 |

Reach-count is the primary gate (±2/16 within the doc's stated single-sample variance); quality
columns (mElaps/mSpeed/bumps/wrongT/hinder) must not materially regress. `hybrid-hier` must not
panic (the `saturating_sub` fix from the sweep stays exercised).

---

## Step-by-Step Tasks

### T1: `BrainContext.goal_override` (per-tick goal injection)

**File**: `crates/brain/src/brains/core.rs`, callers in `crates/qbots/src/main.rs`.

**What to do**: Add `pub goal_override: Option<NavGoal>` to `BrainContext`. In `MainBrain::tick`,
prefer `ctx.goal_override` over the FSM/item/roam goal ladder when `Some` (same precedence the
static knob had). Remove `goal_override` from `BrainConfig` (keep `combat_enabled`). Update the
`bot_task` `BrainContext` construction to pass `goal_override: None`. Unit-test: a brain with a
`Some(goal_override)` navigates to that goal (assert via a stub `Navigator` capturing
`set_goal`). Commit `task(T1): BrainContext.goal_override per-tick; drop BrainConfig.goal_override`.

### T2: `RuntesterBrain` — verbatim lift of the scenario tick

**File**: `crates/brain/src/brains/runtester.rs` (new), `crates/brain/src/brains/mod.rs`, `lib.rs`.

**What to do**: Create `RuntesterBrain` implementing `trait Brain`. Lift `scenario.rs:358–467`'s
decision/steer/recovery body **verbatim** into `tick`, converting the loop locals to brain
fields: `steering: Steering` (`Steering::new(3.0)`), `recovery: Recovery`, `backoff_ticks: u32`,
`escape_yaw: Option<f32>`, `last_serverframe: Option<i32>`. `set_map` is a no-op (it drives the
injected nav). `tick` reads `ctx.goal_override` (the harness pins it), calls `nav.update` /
`nav.set_goal` / `nav.smooth_with_cm` / `pursue_target_safe` exactly as the harness does today,
returns `BrainOutput { intent, weapon_request: None }`. `status()=="runtester"`. Add
`BrainKind::Runtester` + a `build_brain` arm + a `brain_tag` arm. Commit
`task(T2): RuntesterBrain (verbatim lift of scenario tick) + BrainKind::Runtester`.

### T3: CI determinism tests (pin the lift)

**File**: `crates/brain/src/brains/runtester.rs` (tests module).

**What to do**: Deterministic unit tests over a **synthetic** `CollisionModel` (a straight
corridor / an inside corner — reuse any existing test-CM helper in `world`/`brain`, else build a
minimal box) + a **stub `Navigator`** with scripted returns. Assert `RuntesterBrain::tick`:
- steers `forward > 0` toward a scripted `pursue_target_safe` look-ahead;
- drives the goal passed via `ctx.goal_override` (stub captures `set_goal`);
- presses `jump` when the stub reports `current_edge_is_jump()`;
- enters the backoff/escape path when the stub reports no progress (Recovery → `BackOffThenRepath`
  sets `backoff_ticks` and steers along `escape_yaw`);
- applies `arrive` / `speed_scale` throttle (stub `speed_scale` < 1.0 scales `forward` down);
- **never** sets `weapon_request`.
These pin the lift in CI without a live server. Commit `task(T3): RuntesterBrain determinism unit tests`.

### T4: Migrate `scenario.rs` onto `Box<dyn Brain>`

**File**: `crates/qbots/src/scenario.rs`, `crates/qbots/src/main.rs`.

**What to do**: Replace the inline decision/steer/recovery block (`358–467`) with: build
`brain = build_brain(brain_kind, skill, BrainConfig { combat_enabled: false })` once;
`brain.set_map(BrainMap{..})`; per tick assemble `BrainContext { view, nav:
Some(nav_driver.as_mut()), cm: Some(&cm), dt, ticks, goal_override: Some(NavGoal::Position(goal.into())) }`,
`let out = brain.tick(ctx)`, feed `out.intent` into the existing `MovementController`. Delete the
now-dead locals (`steering`/`recovery`/`backoff_ticks`/`escape_yaw`/`last_serverframe` and the
lifted body). Thread `brain_kind: BrainKind` from the CLI (`--brain`, Plan 25), defaulting to
`Runtester` in `main.rs` for `spawn-to-spawn`/`spawn-to-weapon`. Recorder/goal/exit plumbing
unchanged. **Closes Plan 22 T4 + retires Plan 15 duplication.** Commit
`task(T4): scenario.rs drives Box<dyn Brain> (default runtester); delete inline duplication`.

### T5 (optional, P1): reusable sweep aggregator

**File**: `crates/tools/` (new binary `mode-perf-report`).

**What to do**: A `tools` binary that parses per-bot `# SUMMARY` lines from
`logs/<scenario>/*.log` and prints the `mode_perf.md` table shape grouped by navmode (reached /
mElaps / mSpeed / bumps / wrongT / hinder). Run as `cargo run -p tools -- mode-perf-report logs/`.
Makes the 6-navmode comparison repeatable, not hand-read (no tmp scripts). If it balloons, defer
to a follow-up (T6 can read logs manually). Commit `task(T5): tools mode-perf-report log aggregator`.

### T6 (ACCEPTANCE): live 6-navmode sweep vs `mode_perf.md`

**File**: tracker, `context/mode_perf.md`, `context/brain_notes.md`, `SERIES.md`.

**What to do**, against a live q2dm1 server, default `--brain runtester`:
```bash
cargo build -p qbots
for m in astar navmesh hybrid-fallback hybrid-race hybrid-hier hybrid-segment; do
  ./target/debug/qbots spawn-to-spawn                 --count 16 --max-secs 180 --navmode "$m" --name "s2s_$m"
  ./target/debug/qbots spawn-to-weapon rocketlauncher --count 16 --max-secs 180 --navmode "$m" --name "s2w_$m"
done
```
**Gate**: every navmode's reach-count ≥ baseline − 2/16 on both scenarios; quality not materially
worse; `hybrid-hier` no panic. Aggregate with T5 (or read logs). Append a **dated post-refactor
comparison** (baseline vs runtester-brain columns) to `context/mode_perf.md` and a **Plan 26
section** to `context/brain_notes.md` (runtester split, goal_override move, sweep result). If any
navmode regresses past the gate, diff `RuntesterBrain::tick` against the old `scenario.rs` body —
the lift wasn't faithful — before closing. `git mv` plan + tracker to `completed/`, mark
`SERIES.md` Plan 26 done + Plan 22 T4 closed (Rule C). Commit `task(T6): 6-navmode acceptance sweep + close Plan 26` (docs only — sweep is verification).

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/runtester.rs` (new) | `RuntesterBrain` (verbatim lift) + determinism tests | P0 |
| `crates/brain/src/brains/core.rs` | add `BrainContext.goal_override`; drop `BrainConfig.goal_override` | P0 |
| `crates/brain/src/brains/mod.rs` | `BrainKind::Runtester` + factory + `brain_tag` | P0 |
| `crates/qbots/src/scenario.rs` | drive `Box<dyn Brain>` (default runtester); delete `358–467` | P0 |
| `crates/qbots/src/main.rs` | spawn-to-* default `--brain runtester`; `bot_task` ctx `goal_override:None` | P0 |
| `crates/tools/` | `mode-perf-report` aggregator (optional) | P1 |
| `context/mode_perf.md` | dated post-refactor comparison | P0 |
| `context/brain_notes.md` | Plan 26 section | P0 |
| `SERIES.md`, tracker | series update + progress | P1 |

**Reused (do not reimplement):** `pursue_target_safe`, `find_best_direction`,
`Recovery`/`RecoveryAction`, `Steering`, `move_from_world_dir`, `MovementController`,
`Navigator`/`build_navigator`, `MovementRecorder`/`finalize`, `resolve_goal`/`farthest_reachable_spawn`.

---

## Open Questions / Risks

1. **A navmode regresses below the gate.** The biggest risk. *Mitigation*: T2 is a verbatim lift
   driven through the same `Navigator`; if a mode drops, the lift edited something — diff against
   `scenario.rs:358–467` before closing. Do not ship a known regression as "done".
2. **Live-server + variance.** The sweep is high-variance, single-sample, server-dependent.
   *Mitigation*: ±2/16 tolerance; re-run a suspect mode rather than chase noise; keep CI gates
   (T3 + unit/clippy/build) as the merge bar, T6 as a manual acceptance pass logged in the tracker.
3. **`goal_override` precedence drift in MainBrain.** Moving the knob from config to context must
   keep MainBrain's goal precedence identical. *Mitigation*: T1 unit test; `connect-one` still runs.
4. **`--brain`/`--navmode` ordering.** Depends on Plan 25's rename + `--brain` wiring landing
   first. *Mitigation*: dependency is declared; if Plan 25 slips, T4 can temporarily hardcode
   `BrainKind::Runtester` and add the flag when Plan 25 lands.

---

## Verification Checklist

- [ ] T1: `BrainContext.goal_override` added; `BrainConfig.goal_override` removed; override unit test green.
- [ ] T2: `RuntesterBrain` verbatim lift; `BrainKind::Runtester` builds via `build_brain`; clippy clean.
- [ ] T3: determinism tests pass — steers to look-ahead, honors `goal_override`, jumps on jump-edge, backoff/escape on no-progress, throttles, no `weapon_request`.
- [ ] T4: `scenario.rs` drives `Box<dyn Brain>` (default `runtester`); inline `358–467` deleted; Plan 22 T4 closed.
- [ ] Single-bot parity: `spawn-to-spawn`/`spawn-to-weapon` SUMMARY identical to pre-Plan-26 run (±1 tick).
- [ ] T5 (optional): `mode-perf-report` aggregates logs into the comparison table.
- [ ] T6: **all 6 navmodes** ≥ `mode_perf.md` baseline − 2/16 on both scenarios; no quality regression; `hybrid-hier` no panic.
- [ ] T6: dated comparison appended to `context/mode_perf.md`; **Plan 26 section appended to `context/brain_notes.md`**.
- [ ] `--brain main` A/B path runs; `connect-one` unaffected.
- [ ] `cargo build` + `clippy -- -D warnings` + `test` + `fmt` clean throughout; committed each task (Rule A/B/C); plan moved to `completed/`.
