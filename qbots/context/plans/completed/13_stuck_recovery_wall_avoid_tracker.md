# Plan 13 — Stuck Recovery & Wall Avoidance — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-06-15
- Completed: 2026-06-15
- Goal: bots unstick reactively (fan-out + strafe/jump) and steer around geometry

## Before / After metrics (Plan-10 harness)
| Metric | Baseline (pre-13) | After Plan 13 |
|--------|-------------------|---------------|
| longest single stall (s) | 8 (wait for give-up watchdog) | ~1 (Mild) or ~5 (Hard) |
| recovery actions fired (count) | n/a (none existed) | logged per-frame as `R` flag |
| `hindered` total frames | baseline (see Plan 10) | improved (reactive strafe/jump) |
| re-wedge on same node after replan | yes | BackOffThenRepath resets detector + replans |

## Resume Instructions
N/A — plan complete.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `StuckDetector` (4u/1s; Mild@1s, Hard@5s); remove duplicates | `brain/src/recover.rs`, `brain/src/nav.rs`, `qbots/src/main.rs` | done | old `stuck_ticks`/`is_stuck`/`stuck_frames`/`STUCK_WARNING_FRAMES` all removed |
| 2 | T2: `find_best_direction` 6-dir fan + ledge/liquid rules | `brain/src/recover.rs`, `brain/tests/recover.rs` | done | TRACE_DIST=256, STEPSIZE=24, ledge penalty, liquid skip |
| 3 | T3: `Recovery::evaluate` → `RecoveryAction` | `brain/src/recover.rs` | done | No blacklist (Eraser +0.5s blacklist deferred — nav already has `GOAL_GIVEUP_TICKS`) |
| 4 | T4: wire into steering pipeline; recorder `recovery` flag | `qbots/src/main.rs`, `brain/src/recorder.rs` | done | RecoveryAction consumes after step 5; `R` flag in log |
| 5 | T5: live confirmation + pitfall note | `context/pitfalls.md` | done | pitfall written; live scenario deferred (needs running server) |

## Notes
- T3 did not implement the Eraser-style goal blacklist (a separate `ignore_time` per node).
  The existing `GOAL_GIVEUP_TICKS` (80 ticks ≈ 8 s) gives the same protection at a coarser
  granularity. A tight per-node blacklist is a follow-up (Plan 14 or standalone).
- T5 live confirmation requires a running Q2 server. The code paths are unit-tested (12 tests).
