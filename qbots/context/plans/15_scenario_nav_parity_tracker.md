# Plan 15 — Scenario Nav Parity — Tracker

## Overview
- Status: **0% complete**
- Start date: 2026-06-15
- Goal: `spawn-to-spawn` exits with `reached=1`

## Resume Instructions
Read `context/plans/15_scenario_nav_parity.md` for full task details.
All changes are in `crates/qbots/src/scenario.rs` only.
Run `cargo build && cargo clippy -- -D warnings && cargo test` after each task.
Live verification requires a q2dm1 server on localhost:27910.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: seed_spawns + detect_jump_edges + component diagnostic | `qbots/src/scenario.rs` | pending | Mirror supervisor.rs:79-90; mutate before Arc wrap |
| 2 | T2: smooth_with_cm in tick loop | `qbots/src/scenario.rs` | pending | After set_goal; cm is already in scope |
| 3 | T3: jump on jump-link edges | `qbots/src/scenario.rs` | pending | `if nav_driver.current_edge_is_jump() { mv.jump(); }` |
| 4 | T4: Recovery integration | `qbots/src/scenario.rs` | pending | Instantiate Recovery before loop; evaluate + apply actions; wire force_replan on BackOff |
| 5 | T5: Live verification | live server q2dm1 | pending | 2+ of 3 runs reached=1 |

## Baseline (pre-fix)

| Run | mean_speed | hindered | reached | path_efficiency | Root cause |
|-----|-----------|---------|---------|-----------------|------------|
| 1781565937 | 61 u/s | 4 | 0 | 0.869 | Wrong-direction path (missing seed_spawns) |
| 1781566280 | 27 u/s | 130 | 0 | 1.000 | Orbit at wp 833 with no recovery (missing Recovery) |

## Expected Post-Fix

| Metric | Expected |
|--------|----------|
| reached | 1 |
| mean_speed | > 80 u/s (smooth path + no orbit stalls) |
| hindered_frames | < 20 |
| path_efficiency | > 0.85 |
