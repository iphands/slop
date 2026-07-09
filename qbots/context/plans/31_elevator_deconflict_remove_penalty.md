# Plan 31 — Elevator/plat multi-bot de-conflict + remove the lift-penalty hack

> **Status**: pending (file authored 2026-07-09; SERIES row existed since Plan 23 split)
> **Created**: 2026-07-09
> **Depends on**: Plan 43 (single-bot ride behavior), Plan 46 (shared traversal executor)
> **Goal**: Bots use lifts like humans in a *crowd* — wait clear when the plat is up or occupied, ride, step off promptly, back off and retry when another bot holds it — so the `ELEVATOR_PENALTY`/`--lift-penalty` hack can be **deleted** and `context/elevator_todo.md` closed.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Single-bot lift riding works (Plan 43). Multi-bot doesn't: bots pile onto the
pad, `Touch_Plat_Center` re-arms the go-down timer every tick a body is in the shaft, the
lift pins at the top, and the bottom queue starves (full mechanics:
`context/elevator_todo.md` + `pitfalls.md` "func_plat elevator deadlock"). We dodge it
today with a 5000u A* cost on every lift edge (`ELEVATOR_PENALTY`, `--lift-penalty`) that
makes bots take stairs even when a lift is the human choice. Implement the de-conflict,
then delete the hack (it's a tracked debt item; code tags `TODO(elevator-hack)`).

**Deliverables**:
1. Wait-clear behavior: approaching bot holds **outside the shaft footprint** until the
   plat is at the bottom AND unoccupied; prompt step-off at the top (no pad dwell).
2. Occupancy de-conflict: if another player/bot is using or waiting closer, back off to a
   standoff point and re-approach (the user's "move away, come back").
3. `ELEVATOR_PENALTY` + `--lift-penalty` removed from **all** sites (world `build.rs`,
   `lib.rs`, `mapcache.rs` Fingerprint, qbots `supervisor.rs`, `scenario.rs`, `main.rs`,
   `tools/navinspect.rs`); `mapcache::VERSION` bumped; `elevator_todo.md` deleted with its
   content folded into `pitfalls.md` history.
4. Live proof: 8-bot fleet on q2dm1 with lifts un-penalized runs 10+ minutes with no lift
   deadlock (lift completes cycles; no bot starves > ~30s at the bottom).

**Estimated effort**: Medium (1 day, live iteration).

## Context

- The full failure analysis, Q2 `func_plat` state machine, and "what done looks like" are
  already written: `context/elevator_todo.md` (authoritative for this plan's acceptance
  criteria 1–4) and `vendor/yquake2/src/game/g_func.c`.
- Plan 43/46 give the mechanism: the traversal executor's ride machine already has
  Approach/Wait/Cross phases and live plat tracking (`ride.rs::platform_present`,
  `ride_phase`). This plan adds the *social* layer: occupancy checks (other player entities
  within the shaft/pad AABB) and standoff/retry.
- Shaft/pad geometry is known at build time (`RideInfo` board/far + the lift model AABB);
  expose what's missing via `RideInfo` rather than re-deriving in the brain.

## Step-by-Step Tasks

### T1: Occupancy + shaft-clear predicates

**File**: `crates/brain/src/ride.rs` (pure helpers)

**What to do**: `shaft_occupied(view, info) -> Option<EntityId>` (any player entity within
the pad/shaft volume, excluding self) and `plat_at_bottom(view, info) -> bool` (live origin
vs bottom z, tolerance). Unit-test with synthetic worldviews. Extend `RideInfo` with the
pad half-extents if the current fields can't bound the volume (world change → cache
`VERSION` bump).

### T2: Wait-clear + prompt step-off in the ride machine

**File**: `crates/brain/src/traverse.rs` (Plan 46 executor; else `runtester.rs`+`main.rs`)

**What to do**: In the vertical-ride path: Approach holds at a standoff point (board point
projected ~64u outside the shaft) while `!plat_at_bottom || shaft_occupied`; board only
when clear; at the top, immediately steer to the dismount node (the executor already exits
on edge completion — verify no dwell frames on the pad, which is what re-arms the timer).

### T3: Back-off/retry de-conflict

**File**: `crates/brain/src/traverse.rs`

**What to do**: If the wait exceeds a budget (~6s) because another bot holds the lift,
back off to a farther standoff (or a nearby roam node), wait 2–4s (jittered per bot to
break symmetry), re-approach. Cap retries (~3) then ask the navigator to re-plan (A* will
find stairs where they exist). Log `lift_yield` events.

### T4: Delete the hack

**Files**: `crates/world/src/build.rs`, `lib.rs`, `mapcache.rs`, `crates/qbots/src/
{supervisor,scenario,main}.rs`, `crates/tools/src/bin/navinspect.rs`,
`context/elevator_todo.md`, CLAUDE.md/README mentions of `--lift-penalty`

**What to do**: Remove `ELEVATOR_PENALTY`, the `--lift-penalty` flag, and every
`TODO(elevator-hack)` tag; lift edges get their honest cost (ride length + small wait
estimate). Bump `mapcache::VERSION` (edge costs change); regen caches. Update docs that
tell users to pass `--lift-penalty 0` (Plan 43 validation commands).

### T5: Live soak + notes

**What to do**: q2dm1, 8-bot `main` fleet, 10 min: lift cycles complete; no bot waits
> 30s at the bottom; frag flow normal. Then q2dm3 `spawn-to-weapon railgun --instance 1
--count 4` still ≥ 3/4 (no `--lift-penalty` flag anymore). Append `brain_notes.md`;
fold the deadlock post-mortem into `pitfalls.md` and delete `elevator_todo.md`.

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green.
> T4's flag removal and VERSION bump land in one commit.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/ride.rs` | occupancy/at-bottom predicates | P0 |
| `crates/brain/src/traverse.rs` | wait-clear, step-off, back-off/retry | P0 |
| `crates/world/src/build.rs` + qbots/tools call sites | delete penalty + flag | P0 |
| `crates/world/src/mapcache.rs` | VERSION bump | P0 |
| `context/elevator_todo.md` → `pitfalls.md` | close the debt item | P1 |

## Open Questions / Risks

1. **Symmetric standoffs deadlock** (two bots yielding to each other). *Mitigation*:
   jittered waits + retry cap + re-plan fallback.
2. **Deleting the penalty changes ALL cached graphs** — stale caches would silently keep
   penalized costs. *Mitigation*: VERSION bump forces regen; verify with `navinspect`.
3. **Occupancy visibility**: the other bot may be outside our PVS while holding the lift
   top. *Mitigation*: the wait-budget/retry path (T3) covers unseen holders; occupancy is
   an optimization, the timeout is the guarantee.
4. **Plan 46 sequencing** — T2/T3 want the shared executor. *Mitigation*: hard-depend on
   46; do not implement twice.

## Verification Checklist

- [ ] T1: predicates unit-tested; commit.
- [ ] T2: single bot: zero pad-dwell frames at top (log proof); still rides q2dm3 lift; commit.
- [ ] T3: two-bot lift contention resolves (both eventually up; `lift_yield` logged); commit.
- [ ] T4: `grep -r "lift_penalty\|ELEVATOR_PENALTY\|elevator-hack"` returns nothing;
      VERSION bumped; caches regen; commit.
- [ ] T5: 10-min 8-bot soak, no deadlock; q2dm3 railgun ≥ 3/4; notes appended;
      `elevator_todo.md` gone; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
