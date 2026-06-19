# Plan 43 — Ride behavior + q2dm3 reach proof & navmode ranking — Tracker

## Overview
- Status: 60% complete — infrastructure + lift-riding done; reliable train-riding + proof pending
- Start date: 2026-06-19
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
| 4 | T4: recorder `P` flag | `recorder.rs` | pending | deferred (diagnostic) |
| 5 | T5: live q2dm3 reach proof | (live) | blocked | railgun: train-ride timing; quad: Plan 35 |
| 6 | T6: navmode ranking + brain_notes + pitfalls | `context/*.md` | partial | notes written; ranking pending reach |

## Next steps (to finish)
1. **Stateful train boarding** (a `Riding` flag in the brain): once boarded, hold/match the train
   until it nears the far corner (`far_ent` matched), then step to dismount — don't steer to the
   dismount the whole time, and don't revert to `Approach` after leaving the board point.
2. **Plan 35** (broad q2dm3 floor connectivity) to make the quad reachable.
3. Then the live proof + 6-navmode ranking (T5/T6).
