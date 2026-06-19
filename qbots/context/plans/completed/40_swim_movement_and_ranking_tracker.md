# Swim Movement & Navmode Ranking — Tracker

## Overview
- Status: 100% complete (T1–T8 done; live proof PASSED — astar reaches the railgun)
- Start date: 2026-06-19
- Goal metric: live `spawn-to-weapon railgun` `reached=1` on `astar` (and A*-backed
  hybrids); full six-navmode ranking table.

## Resume Instructions
1. Re-read `RULES.md`, `40_swim_movement_and_ranking.md`, and **finish Plan 39 first**
   (this plan needs swim nodes/edges in the graph).
2. Confirm no brain sets `intent.up` yet: `grep -rn "\.up" crates/brain/src`.
3. Water physics ground truth: `vendor/yquake2/src/common/pmove.c` `PM_WaterMove` (`:545`)
   + `PM_CheckSpecialMovement` water-jump (`:414-426`) + waterlevel calc (`:765-790`).
4. Mirror `current_edge_is_jump` consumption (`runtester.rs:174`, `main.rs:401`) for swim.
5. Commit per task; T7 needs a live q2dm1 server.

## Baseline (pre-fix)
- No brain sets `intent.up`; `jump` only sets a one-shot `upmove=270`.
- Recovery avoids water (`recover.rs:158-160`).
- `spawn-to-weapon railgun` currently fatals/fails (goal unreachable — Plan 39 fixes the
  graph; this plan fixes the movement).

## Navmode Ranking (q2dm1, local yquake2 dedicated, --count 1 --max-secs 95, 2026-06-19)

| navmode | reached | elapsed (s) | mean_speed | notes |
|-----------------|:-------:|:-----------:|:----------:|-------|
| astar           | ✅ | 11–27 | 165–207 | A* graph has the swim route; 46/93 frames `S`, z 238→434 |
| navmesh         | ❌ | 95 (cap) | 185 | no navmesh water (Plan 39 scope; expected) |
| hybrid-fallback | ✅ | 28 | 197 | A* primary → plans the swim directly |
| hybrid-race     | ✅ | 40 | 222 | plans both; the A* (swim) plan wins |
| hybrid-hier     | ✅ | 18 | 212 | navmesh corridor + A* local; A* local finds water |
| hybrid-segment  | ❌ | 95 (cap) | 185 | navmesh corridor mis-routes (railgun room navmesh-isolated); selector made swim-aware but insufficient alone |

**4/6 reach** — every A*-driven mode swims the railgun route. See `context/mode_perf.md`.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: water-state helper | `brain/src/water.rs` | done | feet/mid/eye sample; unit-tested |
| 2 | T2: swim movement (intent.up + pitch) | `runtester.rs`, `main.rs` | done | sustained, not jump; combat-aim gated in main |
| 3 | T3: water-exit / surfacing | `runtester.rs`, `main.rs` | done | look-up+fwd water-jump + hysteresis |
| 4 | T4: water-aware recovery | `runtester.rs`, `main.rs` | done | recovery suspended while swimming (caller-side gate) |
| 5 | T5: recorder `S` flag | `recorder.rs` | done | schema doc updated; wired in scenario |
| 6 | T6: build + unit tests | brain tests | done | StubNav swim ascend/exit + descend |
| 7 | T7: live proof + ranking sweep | CLI | done | 4/6 reach; astar 46/93 S-frames; hybrid-segment swim-aware |
| 8 | T8: brain_notes + distilled + close-out | `context/*`, `SERIES.md` | done | Rule C, move 39+40 |
