# Plan 43 — Moving-platform & lift ride behavior + q2dm3 reach proof & navmode ranking

> **Status**: pending
> **Created**: 2026-06-19
> **Depends on**: Plan 42 (ride edges), Plan 41 (spawn-to-item/instance), Plan 40 (swim-movement pattern), Plan 26 (runtester brain)
> **Goal**: Make the brain actually *ride* q2dm3's moving platforms (`func_train`) and the railgun elevator (`func_plat`) — approach, board, ride, dismount — then prove `spawn-to-item quaddamage` and `spawn-to-weapon railgun` reach, ranked across navmodes.
> **Agent**: TBD

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Plan 42 gives A* a `Ride` edge; this plan makes the brain *execute* it. When the
current path edge is `Ride`, the bot walks to the board point, **waits** for the platform to
arrive (read its live origin from frames), steps on, **rides** without fighting it, then steps/
jumps off at the dismount node. Same FSM-suspension discipline as swimming (Plan 40). Then run
the user's exact validation commands and rank navmodes in `context/mode_perf.md`.

**Deliverables**:
1. Live platform tracking: classify q2dm3 inline-model movers (`func_train`/`func_plat`) from
   PVS entities; expose their current origin to the brain (`brain::ride` module).
2. Ride execution in `MainBrain` (and the `runtester` path): approach→wait→board→ride→dismount
   intents, driven by `RideInfo` from the current nav edge + the platform's live position.
3. Recovery suspended while riding/waiting (don't "unstick" a bot standing on a moving plat);
   recorder flag (`P` for plat/ride, mirroring `S` for swim).
4. **Proof**: live q2dm3 `spawn-to-item quaddamage` and `spawn-to-weapon railgun --instance 1`
   reach (`reached=true`) on the A*-backed navmodes; a 6-navmode ranking recorded in
   `context/mode_perf.md`; `context/brain_notes.md` appended (brain-notes discipline).

**Estimated effort**: Large (uncertain; live iteration on a server).

## Context

### What "ride" requires that swim/lift didn't

- A `func_train` **moves in XY**. The board point is fixed (a `path_corner` endpoint), but the
  platform is only *there* part of the time. The bot must **wait** at the board node until the
  platform's live origin is within tolerance, then step on — exactly the user's "hop on the
  moving platform" timing. Live origin comes from `Worldview` PVS entities (the platform is an
  inline-model entity; `perception.rs` already carries entity `origin`/`velocity`).
- While riding, the bot should **not** steer toward the next node (it would walk off) and must
  **not** trigger stuck-recovery (it isn't stuck; it's being carried). Suspend both, like
  swimming (`brains/main.rs:372` `swim_active` gate).
- At the dismount end, step/jump toward the dismount node (use a `Jump` if dz>step), then
  resume normal nav.

### Reuse / precedent

- **Swim execution** (`brains/main.rs:372-426`, `brain::water`) is the structural template:
  detect "edge is X" via `nav.current_edge_is_*()`, override intent, suspend recovery, set a
  recorder flag.
- **Edge kind plumbing**: add `current_edge_is_ride()` + `current_ride_info()` to the
  `Navigator` trait + impls (`nav.rs`, `hybrid/*`, `nav_mode.rs`), mirroring
  `current_edge_is_swim()` (Plan 39/40).
- **Lift riding** (func_plat) is the simpler sub-case (vertical, no XY wait): fold it in here so
  the railgun elevator works and we can **drop the `lift_penalty` hack** for q2dm3-style
  single-bot scenarios. (Full multi-bot de-conflict / `ELEVATOR_PENALTY` removal stays Plan 31;
  note overlap.)

### Scope boundary vs Plan 31

Plan 31 owns the *multi-bot* lift de-conflict (the func_plat deadlock, removing the global
`ELEVATOR_PENALTY`). This plan delivers *single-bot* ride competence (the scenario lens). Where
they overlap (wait-clear / step-off), implement once in `brain::ride` and let Plan 31 build the
de-conflict on top. Do **not** remove the global penalty here — keep scenario runs using
`--lift-penalty 0` to exercise lifts.

## Step-by-Step Tasks

### T1: `brain::ride` — live platform tracking

**File**: `crates/brain/src/ride.rs` (new), `crates/brain/src/lib.rs`

**What to do**: Given the `Worldview` and a `RideInfo` (board/dismount/wait + model_index),
find the matching mover entity by proximity to the board/wait point (inline-model entities have
no friendly class on the wire; match by nearest entity to the expected path) and return its
live origin + whether it's "at the board point" within tolerance. Pure function; unit-test with
synthetic `Worldview`s.

### T2: `Navigator::current_edge_is_ride()` + `current_ride_info()`

**Files**: `crates/brain/src/nav.rs`, `crates/brain/src/nav_mode.rs`,
`crates/brain/src/hybrid/{fallback,race,hier,segment}.rs`

**What to do**: Mirror `current_edge_is_swim()`: expose whether the current path edge is `Ride`
and surface its `RideInfo`. Default impls return `false`/`None` so navmesh-only backends compile.

### T3: ride execution in `MainBrain`

**File**: `crates/brain/src/brains/main.rs`

**What to do**: When `nav.current_edge_is_ride()`:
- **Approach**: steer to `wait_at` (board node) as normal.
- **Wait**: if at `wait_at` but platform not at board point → hold position (no forward), face
  the platform's approach.
- **Board/ride**: once the platform's live origin is within tolerance, step onto it; while the
  bot's feet are on the moving model, zero the nav-forward (let it carry), keep balance.
- **Dismount**: when the platform nears the dismount end, press forward (+ jump if dz>step)
  toward `dismount`, then clear the ride state. Suspend stuck-recovery the whole time (extend
  the `swim_active` gate to `ride_active`). Add a recorder `P` flag.

### T4: recorder `P` flag + runtester parity

**Files**: `crates/brain/src/recorder.rs`, `crates/brain/src/brains/main.rs` (shared with
runtester via the common tick), verify `RunTesterBrain` inherits the ride path.

**What to do**: Add `riding: bool` to the recorder frame + `P` flag char. Ensure the scenario
brain (`runtester`) drives rides too (the `spawn-to-*` lens uses it by default).

### T5: live proof — q2dm3 reach

**What to do** (live, server on q2dm3):
```bash
cargo run --release --bin qbots -- spawn-to-item quaddamage --count 4 --max-secs 150 --navmode <mode> --lift-penalty 0
cargo run --release --bin qbots -- spawn-to-weapon railgun --instance 1 --count 4 --max-secs 150 --navmode <mode> --lift-penalty 0
```
Confirm `reached=true` on `astar` first (the reference), then the A*-backed hybrids. Capture
logs (`./logs/spawn-to-item/…`, `…/spawn-to-weapon/…`); confirm `P` frames appear (bot was
carried) and z-profile shows the ride + elevator ascent.

### T6: navmode ranking + knowledge capture

**Files**: `context/mode_perf.md`, `context/brain_notes.md`, `context/pitfalls.md` (if a gotcha)

**What to do**: Run all 6 navmodes for both goals on q2dm3; record a reach table (like Plan 40's
ranking) in `mode_perf.md` with a dated q2dm3 section. Append a dated `brain_notes.md` section
(mandatory per SERIES brain-notes discipline). Note any ride-timing pitfalls in `pitfalls.md`.

> **Rule B reminder**: commit after *each* task. fmt + clippy(-D warnings) + tests green before
> every commit. Live-proof tasks commit the captured ranking/notes, not logs (`./logs` gitignored).

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/ride.rs` | live platform tracking (new) | P0 |
| `crates/brain/src/brains/main.rs` | approach/wait/board/ride/dismount + recovery suspend | P0 |
| `crates/brain/src/nav.rs`, `nav_mode.rs`, `hybrid/*` | `current_edge_is_ride`/`current_ride_info` | P0 |
| `crates/brain/src/recorder.rs` | `riding`/`P` flag | P1 |
| `context/mode_perf.md`, `context/brain_notes.md` | q2dm3 ranking + notes | P1 |

## Open Questions / Risks

1. **Inline-model platform has no friendly class on the wire** — matching the right entity is
   by-position. *Mitigation*: T1 matches nearest mover to the known path; the path geometry is
   static and known from the BSP, so ambiguity is low (q2dm3 has 3 trains in distinct areas).
2. **Board timing is hard** — board too early/late = miss or fall. *Mitigation*: generous board
   tolerance; wait until live origin within radius AND low relative speed; jump-assisted board.
3. **PVS**: the platform may not be in the bot's PVS until close. *Mitigation*: approach the
   board node first (it's a normal Walk edge); the platform enters PVS as the bot nears it.
4. **Recovery/anti-orbit fighting the ride** (the reason for the lift penalty). *Mitigation*:
   the `ride_active` suspension (T3) is the core fix; verify no recovery `R` frames during `P`.
5. **Other navmodes (pure navmesh) have no ride edges** — they will fail the goal, as with
   water (Plan 40). *Mitigation*: expected; record as such in the ranking, don't treat as a bug.

## Verification Checklist

- [ ] T1: `brain::ride` unit tests match a mover by position from a synthetic `Worldview`.
- [ ] T2: all `Navigator` impls expose `current_edge_is_ride`/`current_ride_info` (compile + default).
- [ ] T3: on a `Ride` edge the bot waits, boards, rides (nav-forward zeroed), dismounts; no `R` during `P`.
- [ ] T4: recorder emits `P`; `runtester` drives rides (scenario default).
- [ ] T5: live q2dm3 `spawn-to-item quaddamage` `reached=true` (astar); `spawn-to-weapon railgun
      --instance 1` `reached=true` (astar) — `P` frames present, z-profile shows ride+elevator.
- [ ] T6: 6-navmode q2dm3 ranking in `mode_perf.md`; `brain_notes.md` appended (dated).
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
