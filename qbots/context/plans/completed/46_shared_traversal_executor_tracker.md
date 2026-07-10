# Plan 46 — Shared traversal executor — Tracker

## Overview
- Status: **100% complete (2026-07-09).** All code shipped (T1–T5) + the full live matrix passed on
  both maps. Plan + tracker moved to `completed/`.
- Start date: 2026-07-09
- Goal: one `brain::traverse::TraversalExecutor` (ladder/swim/ride) used by runtester, main,
  AND q3 — q3 gains all traversal, main gains ladders, duplicates deleted.

## Live results (q2dm3, noir.lan:27910)
- **T2 runtester** ride regression: `spawn-to-weapon railgun --instance 1 --count 4 astar` = **4/4**
  (≥ 3/4 baseline). Train + lift + boarded-carry lock all via the executor.
- **T3 main** (gained ladders + stateful ride): `--brain main` reaches (34.8s), **77 `P` frames,
  0 `P`+`R` frames** (recovery suspended during traversal, as the gate guarantees).
- **T4 q3** (gained ALL traversal): `--brain q3` reaches (1/3, rides `*3/*4` + `*2` lift) — was
  structurally 0/N before (no traversal at all).

## Live results (q2dm1 swim, noir.lan:27910) — 2026-07-09
`spawn-to-weapon railgun --count 3 astar` (the water-room railgun; route = 8 swim edges):
**runtester 3/3, main 3/3, q3 3/3.** q3 3/3 is the headline — it had NO swim capability before.
The swim machine is byte-preserved (runtester 3/3) and gained by main/q3 unchanged. **T2/T4 swim
legs closed.**

## Resume Instructions
1. Read `46_shared_traversal_executor.md`. Extraction sources (lift the BEST copy verbatim):
   ladder + stateful train machine from `brains/runtester.rs:250-399`; swim from
   `brains/main.rs:546-573`. `ride.rs`/`water.rs` stay as pure helpers.
2. Regression gate before touching main/q3: q2dm1 railgun swim, q2dm3 railgun ride (≥3/4),
   q2dm3 quad from spawn3 — all with `--brain runtester`, results must match pre-change.
3. Coordinate the recorder `P` flag with Plan 43 T4 (don't double-add).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: extract `TraversalExecutor` + unit tests | `brain/src/traverse.rs` | done | 5 unit tests; gates()/apply(); TraversalFrame |
| 2 | T2: runtester adopts (regression matrix) | `brains/runtester.rs` | done | q2dm3 ride 4/4; q2dm1 swim 3/3 |
| 3 | T3: main adopts (gains ladders) | `brains/main.rs` | done | ride reaches (77 `P`, 0 `P`+`R`); swim 3/3 |
| 4 | T4: q3 adopts (gains swim/ride/ladder) | `brains/q3/mod.rs` | done | q2dm3 ride reaches; q2dm1 swim 3/3 (was 0) |
| 5 | T5: recorder `S`/`P`/`L` + brain_notes | `recorder.rs`, `scenario.rs`, `context/*.md` | done | `L` split from `P`; notes + pitfalls appended |
