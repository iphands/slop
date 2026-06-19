# Plan 19 — Nav Graph Quality & 8-Bot Fleet Reach Validation

> **Status**: pending
> **Created**: 2026-06-16
> **Depends on**: Plan 17 (STEP fix), Plan 18 (map cache — for fast iteration while tuning),
> Plan 15 (scenario nav parity, done)
> **Goal**: `cargo run --bin qbots -- spawn-to-spawn --count 8 --max-secs 60` and
> `spawn-to-weapon rocketlauncher --count 8 --max-secs 60` both reach 8/8 on a live q2dm1 server.
> **Agent**: implementation agent (ralph-loop)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Close the remaining gaps between "some bots reach, some get stuck near the goal" and
"8/8 reach": apply Plan 17's `STEP` fix project-wide, seed non-spawn scenario goals (weapon
origins) into the nav graph the same way DM spawns already are, add `--max-secs` /
`--count` CLI knobs needed to actually run the user's two target invocations, and produce a
clear per-bot pass/fail summary instead of just an aggregate exit code.

**Deliverables**:
1. `--max-secs <f32>` CLI flag on `spawn-to-spawn` and `spawn-to-weapon` (default 30.0,
   unchanged; lets the user pass `--max-secs 60`).
2. `--count <u8>` on `spawn-to-weapon` (currently hardcoded to `1`), mirroring `spawn-to-spawn`.
3. Scenario goal positions (weapon origins, not just DM spawns) seeded into the nav graph.
4. A per-bot pass/fail summary line printed after a multi-bot scenario run.
5. Live verification: both target commands run 8/8 reach on q2dm1, logged in the tracker.

**Estimated effort**: Small–Medium (half day, plus live-verification iteration time).

---

## Context

### Why bots still got stuck near (not far from) the goal

`context/pitfalls.md`'s nav-fragmentation entry reports "2-3/8 bots reaching, others traveling
1000-2000 units before getting stuck near goal" as a *path-quality* issue distinct from the
*connectivity* issue that bridging fixed. Three concrete, code-level contributors:

1. **`STEP` mismatch** (Plan 17 T1): a height jump the grid graph calls "walkable" but the real
   movement code can't climb in one step — manifests as exactly this "got close, then stuck"
   pattern on stairs/ledges.
2. **Unseeded scenario goals**: `NavigationDriver::set_goal` (`brain/src/nav.rs:97-118`) snaps
   the literal goal position to `nav_graph.nearest(&pos)` — whatever grid node happens to be
   closest, not necessarily *at* the goal. `seed_spawns` (called in `scenario.rs:97`) guarantees
   an exact node at every DM spawn, but `ScenarioGoal::Weapon`'s `resolve_goal`
   (`scenario.rs:415-439`) never seeds the weapon origin itself — only the DM spawns (used as
   the *start*, not the goal, in this mode). The nearest unseeded grid node can be up to ~half
   the grid spacing away and on the wrong side of a doorway/stair lip, which combined with
   `GOAL_TOL=48` (`recorder.rs:55`) is sometimes, but not reliably, close enough.
3. **No per-bot visibility into multi-bot failures**: `run_scenario_cmd` (`main.rs:913-990`)
   already aggregates correctly (`ExitCode::SUCCESS` only if every spawned task succeeds), but
   on a partial failure the operator only sees "exit code 2" with no indication of *which* bot(s)
   failed — slows down the live-verification loop this plan and Plan 17/18 all depend on.

### Why this is a separate plan from Plan 17/18

Plan 17 fixes a vendor-parity bug; Plan 18 is pure infrastructure (no behavior change to
reached/not-reached). This plan is the one that actually closes the user-visible gap and runs
the two target commands live — keep it as the final, outcome-owning plan in the chain so its
verification checklist is the literal acceptance test for this whole effort.

---

## Step-by-Step Tasks

### T1: Audit other `STEP`-adjacent constants after Plan 17

**File**: `crates/brain/src/nav.rs`, `crates/brain/src/recover.rs`, `crates/world/src/navgraph.rs`

**What to do**: `grep -n STEP` across `world`/`brain`. Distinguish, in a code comment at each
site, between **step-climb height** (should track the vendor `STEPSIZE=18`, fixed in Plan 17)
and **unrelated tolerances that happen to also be 24** (e.g. `nav.rs`'s `WP_REACH_DZ = 24.0` —
this is a waypoint-arrival tolerance, a deliberate design choice per its own comment "loosen
slightly... to tolerate step heights on ledges", not a step-climb constant). Do not change
`WP_REACH_DZ` reflexively just because it's also `24.0` — only change constants that are
actually meant to model the same physical quantity as `STEPSIZE`. Document the distinction
inline so a future pass doesn't re-confuse them.

**Commit**: `task(T1): audit STEP-adjacent constants, document step-climb vs arrival-tolerance`

---

### T2: Seed scenario goal positions into the nav graph

**File**: `crates/qbots/src/scenario.rs`

**What to do**: After building `graph_mut` and calling `seed_spawns(&cm, &bsp_spawns)`
(`scenario.rs:97`), also seed the resolved scenario goal position itself when it isn't already
one of the DM spawns — i.e. for `ScenarioGoal::Weapon`, call
`graph_mut.seed_spawns(&cm, &[weapon_origin])` (the existing function takes a slice, so a
single-point call works as-is — no new API needed) right after `resolve_goal` returns the
weapon's origin, before `seed_spawns(&cm, &bsp_spawns)` finishes (or just include the goal
position in the same call's slice). For `ScenarioGoal::FarthestSpawn` this is a no-op (the goal
is always one of `bsp_spawns`, already seeded).

**Commit**: `task(T2): seed scenario goal position (not just DM spawns) into nav graph`

---

### T3: `--max-secs` CLI flag

**File**: `crates/qbots/src/main.rs`

**What to do**: Add `#[arg(long, default_value = "30.0")] max_secs: f32` to both
`Cmd::SpawnToSpawn` and `Cmd::SpawnToWeapon`. Thread it through `run_scenario_cmd` (replacing
the hardcoded `scenario::DEFAULT_MAX_SECS` at the `SpawnToSpawn` call site, `main.rs:962`) down
to `run_scenario`'s `max_secs` parameter. Keep `DEFAULT_MAX_SECS` in `scenario.rs` as the clap
default's source of truth if convenient, or just duplicate the literal in the `default_value`
string — either is fine as long as they can't silently diverge (prefer referencing the const if
clap's `default_value` can take a computed string; otherwise add a `const_assert`-style test).

**Commit**: `task(T3): add --max-secs CLI flag to spawn-to-spawn/spawn-to-weapon`

---

### T4: `--count` on `spawn-to-weapon`

**File**: `crates/qbots/src/main.rs`

**What to do**: Add the same `#[arg(long, default_value = "1")] count: u8` field already on
`Cmd::SpawnToSpawn` to `Cmd::SpawnToWeapon`, and pass it through to `run_scenario_cmd` instead
of the hardcoded `1` literal at `main.rs:1333`.

**Commit**: `task(T4): add --count to spawn-to-weapon (mirrors spawn-to-spawn)`

---

### T5: Per-bot pass/fail summary

**File**: `crates/qbots/src/main.rs` (`run_scenario_cmd`)

**What to do**: Track each bot's name alongside its `ExitCode` in the `handles` loop (currently
just `Vec<JoinHandle<ExitCode>>` — change to carry the `bot_name` too, e.g.
`Vec<(String, JoinHandle<ExitCode>)>`). After all tasks complete, log one summary line per bot
(`tracing::info!(bot = %name, result = ?code, "scenario result")`) and a final aggregate line
(`N/total bots reached`). This doesn't change the exit-code semantics (still first-failure-wins
for the process exit code) — it only adds visibility, which is what the live-verification loop
in T6 actually needs to debug partial failures quickly.

**Commit**: `task(T5): per-bot pass/fail summary in multi-bot scenario runs`

---

### T6: Live verification — the actual goal

**What to do**, against a live server already running q2dm1:
```bash
cargo run --bin qbots -- spawn-to-spawn --count 8 --max-secs 60
cargo run --bin qbots -- spawn-to-weapon rocketlauncher --count 8 --max-secs 60
```
Record the per-bot summary (T5) for each run in the tracker. If fewer than 8/8 reach, use the
per-bot log + the existing `nav graph has multiple disconnected components` /
`spawns_in_largest_component` diagnostics (`scenario.rs:104-115`) to identify whether the
failure is: (a) still a STEP/connectivity gap Plan 17 should have caught — re-open Plan 17; (b)
a goal-seeding gap T2 should have caught — re-open T2; (c) a genuinely new failure mode — add a
new task here rather than declaring the plan done with a known gap. Iterate until 8/8 on both
commands, or until the gap is well enough understood to hand off as a follow-up plan (don't
silently ship a partial result as "done").

**Commit**: none (verification only — update tracker; this task's outcome is the plan's
acceptance criterion).

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/scenario.rs` | T2: seed goal position | P0 |
| `crates/qbots/src/main.rs` | T3, T4, T5: CLI flags + summary | P0 |
| `crates/brain/src/nav.rs`, `crates/brain/src/recover.rs` | T1: constant audit (docs only, maybe no code change) | P1 |

---

## Open Questions / Risks

1. **What if 8/8 still doesn't happen after T1-T5?** This is real research, not a guaranteed
   win — the plan's job is to either reach 8/8 or produce a sharp, evidence-backed diagnosis of
   what's still wrong (e.g. a specific stair geometry, a specific component split) as a
   follow-up plan, rather than papering over it.
2. **Live server dependency**: all of T6 needs a real q2dm1 server reachable at the configured
   address — this can't be verified in CI. Keep T1-T5's unit/clippy/build gates as the
   merge-blocking bar; T6 is a separate, manually-run acceptance pass logged in the tracker.
3. **`--count 8` stagger**: `run_scenario_cmd` already staggers spawns by 500ms
   (`main.rs:949-952`) — if 8 simultaneous nav-graph builds (before Plan 18 lands) saturate CPU
   and cause frame timing issues, that's a Plan 18 concern, not this plan's; don't conflate a
   performance symptom with a navigation-correctness symptom when diagnosing T6 failures.

---

## Verification Checklist

- [ ] T1: constants audited; comments added distinguishing step-climb vs arrival-tolerance
- [ ] T2: weapon goal seeding verified via log (`seeded` count includes the goal point)
- [ ] T3: `--max-secs 60` accepted and respected (scenario runs past 30s without cutting off)
- [ ] T4: `spawn-to-weapon --count 4` spawns 4 bots (verify via per-bot summary, T5)
- [ ] T5: multi-bot run prints one line per bot + an aggregate "N/total reached" line
- [ ] T6: `spawn-to-spawn --count 8 --max-secs 60` → 8/8 reached, logged in tracker
- [ ] T6: `spawn-to-weapon rocketlauncher --count 8 --max-secs 60` → 8/8 reached, logged in tracker
- [ ] `cargo build` + `cargo clippy -- -D warnings` + `cargo test` + `cargo fmt` clean throughout
