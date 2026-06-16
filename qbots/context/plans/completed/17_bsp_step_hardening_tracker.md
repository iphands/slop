# Plan 17 — BSP/Collision Hardening & Step-Size Correctness — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-06-16
- Completed: 2026-06-16

## Resume Instructions
Read `context/plans/17_bsp_step_hardening.md` for full task details. All changes are in
`crates/world/src/{navgraph.rs,bsp.rs,collision.rs}` plus `context/pitfalls.md`.
Run `cargo build && cargo clippy -- -D warnings && cargo test && cargo fmt` after each task.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: fix STEP 24→18 | `world/src/navgraph.rs` | done | commit `766471dc5`; counts unchanged at 64u grid spacing on q2dm1 — see table below |
| 2 | T2: entity comment handling | `world/src/bsp.rs` | done | commit `8ca72ac9e` |
| 3 | T3: vendor constant pin tests | `world/src/collision.rs` | done | commit `555b326fb` |
| 4 | T4: backfill pitfalls.md | `context/pitfalls.md` | done | commit `390ae81fe` |
| 5 | T5: live verification | live/local | done | see notes below |

## q2dm1 nav graph counts (T1)

`cargo run -p qbots -- nav q2dm1` (64u grid spacing):

| | nodes | edges | components | largest component |
|---|---|---|---|---|
| Before (STEP=24) | 1045 | 7610 | 7 | 850 |
| After (STEP=18) | 1045 | 7610 | 7 | 850 |

No change at 64u grid spacing — q2dm1 apparently has no adjacent-node pairs
whose height delta falls in the 19-24u band that the old (wrong) STEP=24
threshold was bridging. The fix is still correct (matches `pmove.c:32`) and
may matter at finer grid spacing or on other maps; not a reason to revert.

## T5 — live verification (2026-06-16)

- `QBOTS_BASE_PATH=vendor/baseq2 cargo run -p tools --bin bsp_verify -- q2dm1`:
  same 1045-node / 7-component (850/133/26/22/9/3/2) output as before T1 — geometry
  parsing unaffected, as expected (bsp_verify uses 64u spacing, same as above).
- `./target/debug/qbots spawn-to-spawn --map q2dm1 --name qb_t17` against the live
  `noir.lan:27910` server (this run uses the finer-spacing augmented nav graph in
  `scenario.rs`, not the 64u grid): `reached=false elapsed=21.26 distance=647
  mean_speed=30 max_speed=300 bumps=29 wrong_turns=1 hindered_frames=125`. This is
  in the same failure mode and magnitude as the Plan 10 baseline
  (`reached=false elapsed=29.92 distance=1002 mean_speed=33 ... bumps=18`) — not a
  regression. Bot connects, navigates, and moves; the known "stuck near goal" /
  fragmentation issue (Plan 19) is unaffected by this plan's changes.
