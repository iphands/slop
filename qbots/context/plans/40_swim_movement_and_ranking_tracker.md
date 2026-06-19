# Swim Movement & Navmode Ranking — Tracker

## Overview
- Status: 0% complete
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

## Navmode Ranking (fill in during T7 — q2dm1, --count 1 --max-secs 300)

| navmode | reached | elapsed (s) | mean_speed | notes |
|-----------------|---------|-------------|------------|-------|
| astar           | TBD | | | A* graph has water (expected reach) |
| navmesh         | TBD | | | no navmesh water (expected fail) |
| hybrid-fallback | TBD | | | A* primary → expected reach |
| hybrid-race     | TBD | | | navmesh corridor may lose water route |
| hybrid-hier     | TBD | | | navmesh global / A* local |
| hybrid-segment  | TBD | | | A* owns jump/swim links |

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: water-state helper | `brain/src/water.rs` | pending | feet/waist/eye sample |
| 2 | T2: swim movement (intent.up + pitch) | `runtester.rs`, `main.rs` | pending | sustained, not jump |
| 3 | T3: water-exit / surfacing | `runtester.rs`, `main.rs` | pending | look-up+fwd water-jump |
| 4 | T4: water-aware recovery | `recover.rs` + callers | pending | gate stuck/water-skip |
| 5 | T5: recorder `S` flag | `recorder.rs` | pending | schema doc |
| 6 | T6: build + unit tests | brain tests | pending | StubNav swim cases |
| 7 | T7: live proof + ranking sweep | CLI | pending | six navmodes |
| 8 | T8: brain_notes + distilled + close-out | `context/*`, `SERIES.md` | pending | Rule C, move 39+40 |
