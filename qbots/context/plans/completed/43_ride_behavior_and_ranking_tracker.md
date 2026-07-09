# Plan 43 — Ride behavior + q2dm3 reach proof & navmode ranking — Tracker

## Overview
- Status: **100% complete (2026-07-09).** T4 (`P` flag) shipped `35cd30643`; T6 six-navmode
  ranking completed live on q2dm3 (`noir.lan:27910`). Railgun rides on every A*-backed navmode
  (astar/hybrid-race/**hybrid-hier** all 3/4); navmesh & hybrid-segment 0/4 (no ride edges).
  Quad from far/random spawns = Plan 35 (accepted). Plan + tracker moved to `completed/`.
- Start date: 2026-06-19

## Remaining work (2026-07-09 revision)
1. **T4 — recorder `P` flag**: add `riding: bool` to the recorder frame + `P` flag char
   (`crates/brain/src/recorder.rs`); set it from the ride gate in `MainBrain` and
   `RunTesterBrain` (mirror the swim `S` flag). Verify no `R` (recovery) frames during `P`.
2. **T6 — complete the ranking**: `mode_perf.md`'s q2dm3 section has astar / hybrid-race /
   hybrid-fallback rows; run the remaining navmodes (navmesh, hybrid-hier, hybrid-segment —
   expected 0, like water) for BOTH goals and record the full table. Quad rows: run from
   spawn3 (`--count 1`) per the user's accepted scope; note the far-spawn caveat → Plan 35.
3. Then `git mv` plan + tracker to `completed/` and mark SERIES done.

## Update 2026-06-19 (T7 — JUMP on/off + live train tracking): RAILGUN REACHED
- `spawn-to-weapon railgun --instance 1 --count 4 --max-secs 150 --lift-penalty 0`:
  **astar 3/4** (32/91/108 s), **hybrid-race 3/4**, hybrid-fallback 1/4 (navmesh has no rides).
  Ranking in `mode_perf.md`; brain_notes appended.
- Fixes that did it: JUMP on board + dismount (user insight); track the train's **live
  top-center** (`entity.origin + (far - far_ent)`) while carried so the bot stays on the moving
  platform; lifts ride as vertical edges. Deaths ~7 → ~1.
- **Remaining**: the **quad** is nav-unreachable (q2dm3 upper-level fragmentation = **Plan 35**),
  so `spawn-to-item quaddamage` can't pass yet. Recorder `P` flag (T4) still deferred.
- Goal: brain rides q2dm3 `func_train` + railgun `func_plat`; `spawn-to-item quaddamage` and
  `spawn-to-weapon railgun --instance 1` reach; 6-navmode ranking recorded.

## Outcome (2026-06-19)
- **`brain::ride`** module + **`Navigator::current_edge_is_ride`/`current_ride_info`** wired
  through `NavigationDriver` + all four hybrid backends + the trait defaults — done, unit-tested.
- **Ride execution** in **both** `MainBrain` and `RunTesterBrain` (the scenario default):
  approach → wait → cross, with stuck-recovery suspended while riding (`ride_active`). Done.
- **Lift riding WORKS**: `func_plat`/`func_door` are vertical ride edges; the bot rides them up.
  Verified live on q2dm3 — the bot reaches z≈393 (upper levels) via lifts, where before it was
  stuck on the lower floor (z-16) trying to "walk" the vertical lift edge.
- **Train detection**: the brush-model train's **wire origin is `corner - mins`** (NOT the
  stand-center) — captured at build time as `RideInfo::board_ent` and matched against live PVS
  entities. Fixed the "wait forever" (proximity to the stand-center never matched).
- **Train board ledges are solid ground** (Plan 42 `nearest_ground`) so the bot no longer walks
  off into the pit while *approaching* (respawns on approach eliminated).
- **STILL NOT REACHING**: riding the *moving* train across the pit isn't reliable — the bot
  boards but is carried off the far edge / mistimes, dying ~6×/110s. Root cause: the Cross phase
  steers continuously toward the dismount, which walks the bot off the moving platform; and the
  distance-to-board phase logic flips back to `Approach` once boarded (the bot drifts away from
  the board point on the train). **Needs stateful boarding** (a `Riding` brain state: board when
  the train is here → ride passively/match the train → step off only when the train reaches the
  far corner). That is the next concrete task.
- **Quad** (`spawn-to-item quaddamage`) is **nav-unreachable** (Plan 42 outcome → Plan 35), so
  its physical run can't pass yet.

## Validation commands (user-provided)
```bash
# Build the q2dm3 cache first (partially-connected map → --allow-failures; lift-preferred):
qbots generate-map-cache --map q2dm3 --spacing 24 --allow-failures --lift-penalty 0
qbots spawn-to-item quaddamage --count 4 --max-secs 150 --navmode <mode> --lift-penalty 0
qbots spawn-to-weapon railgun --instance 1 --count 4 --max-secs 150 --navmode <mode> --lift-penalty 0
```

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `brain::ride` live platform tracking | `brain/src/ride.rs` | done | wire-origin (board_ent) match |
| 2 | T2: `current_edge_is_ride`/`current_ride_info` | `nav.rs`, `nav_mode.rs`, `hybrid/*` | done | |
| 3 | T3: ride execution in `MainBrain` | `brains/main.rs` | done | lifts work; trains need stateful board |
| 3b | ride execution in `RunTesterBrain` (scenario) | `brains/runtester.rs` | done | scenario default |
| 4 | T4: recorder `P` flag | `recorder.rs`, `scenario.rs` | done | `riding` frame field + `P` flag char (phantom moved `P`→`T`); set from `current_edge_is_ride()` in scenario sampler (`35cd30643`) |
| 5 | T5: live q2dm3 reach proof | (live) | done | railgun REACHED (astar/race/hier 3/4); quad from spawn3 (far-spawn→Plan 35) |
| 6 | T6: navmode ranking + brain_notes + pitfalls | `context/*.md` | done | full 6-navmode table in `mode_perf.md`; `brain_notes.md` appended 2026-07-09 |

## Next steps (to finish)
1. **Stateful train boarding** (a `Riding` flag in the brain): once boarded, hold/match the train
   until it nears the far corner (`far_ent` matched), then step to dismount — don't steer to the
   dismount the whole time, and don't revert to `Approach` after leaving the board point.
2. **Plan 35** (broad q2dm3 floor connectivity) to make the quad reachable.
3. Then the live proof + 6-navmode ranking (T5/T6).

## Update 2026-06-19 (T5 closeout): QUAD REACHED — ride solved
- `spawn-to-item quaddamage` (astar) **reaches the quad** (closest=8, ~20-26s) when the bot starts
  on/near the board ledge (spawn3) — the natural human start (rides `*10` over the lava, sits still
  while carried, jumps off onto the quad ledge). `spawn-to-weapon railgun --instance 1` reaches too.
- Three ride bugs fixed by MEASURING (T1 live `QBOTS_OBSERVE_MOVERS` + ride telemetry), not guessing:
  null `[0,0,0]` world entities falsely triggered `train_here`; `boarded` committed before actually
  on the deck; the nav advanced off the ride edge mid-transit (now locked via `active_ride`). See
  `context/brain_notes.md` (2026-06-19) + `context/pitfalls.md` (func_train deck height).
- New diagnostics retained: `QBOTS_OBSERVE_MOVERS=1 connect-one` (live mover log) and the
  `spawn-to-point <x> <y> <z>` scenario (isolate a nav feature).
- **Deferred (user decision):** far-spawn ROUTE reliability to the board (`--count 4` lands ~1/4
  because bots spread across far spawns). Root cause = q2dm3 upper-level nav fragmentation +
  hull-blocked over-long bridge edges (e.g. a 354u "walk" the hull can't traverse). This is a
  substantial nav-graph rebuild = Plan 35 scope; user opted to stop at the proven ride-from-spawn3.
