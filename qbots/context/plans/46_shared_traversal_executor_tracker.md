# Plan 46 — Shared traversal executor — Tracker

## Overview
- Status: 0% complete
- Start date: —
- Goal: one `brain::traverse::TraversalExecutor` (ladder/swim/ride) used by runtester, main,
  AND q3 — q3 gains all traversal, main gains ladders, duplicates deleted.

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
| 1 | T1: extract `TraversalExecutor` + unit tests | `brain/src/traverse.rs` | pending | |
| 2 | T2: runtester adopts (regression matrix) | `brains/runtester.rs` | pending | gate before T3/T4 |
| 3 | T3: main adopts (gains ladders) | `brains/main.rs` | pending | no `R` during traversal |
| 4 | T4: q3 adopts (gains swim/ride/ladder) | `brains/q3/mod.rs` | pending | suppress dodge while active |
| 5 | T5: recorder `S`/`P`/`L` + brain_notes | `recorder.rs`, `context/*.md` | pending | |
