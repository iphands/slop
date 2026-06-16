# Plan 17 — BSP/Collision Hardening & Step-Size Correctness — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-16

## Resume Instructions
Read `context/plans/17_bsp_step_hardening.md` for full task details. All changes are in
`crates/world/src/{navgraph.rs,bsp.rs,collision.rs}` plus `context/pitfalls.md`.
Run `cargo build && cargo clippy -- -D warnings && cargo test && cargo fmt` after each task.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: fix STEP 24→18 | `world/src/navgraph.rs` | pending | record before/after node/edge/component counts below |
| 2 | T2: entity comment handling | `world/src/bsp.rs` | pending | |
| 3 | T3: vendor constant pin tests | `world/src/collision.rs` | pending | |
| 4 | T4: backfill pitfalls.md | `context/pitfalls.md` | pending | |
| 5 | T5: live verification | live/local | pending | |

## q2dm1 nav graph counts (T1)

| | nodes | edges | components |
|---|---|---|---|
| Before (STEP=24) | | | |
| After (STEP=18) | | | |
