# Plan 13 — Stuck Recovery & Wall Avoidance — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-15
- Goal: bots unstick reactively (fan-out + strafe/jump) and steer around geometry

## Before / After metrics (Plan-10 harness)
| Metric | Baseline (pre-13) | After Plan 13 |
|--------|-------------------|---------------|
| longest single stall (s) | | <5 |
| recovery actions fired (count) | n/a (none existed) | many, short |
| `hindered` total frames | | lower |
| re-wedge on same node after replan | yes | no (blacklist) |

## Resume Instructions
1. T1 (unified detector) must **remove** both old detectors (nav.rs + main.rs) in the same
   change to avoid two systems fighting.
2. T2/T3 build on T1; T3's `RecoveryAction` is what the steering controller (Plan 12) consumes.
3. T4 depends on Plan 12's `Steering` existing (recovery feeds it as top priority). If Plan 12
   is partially landed, gate T4 on the parts that exist.
4. Confirm `Arc<CollisionModel>` is reachable from the tick (shared with Plan 11 T2b).
5. `STEPSIZE` (18 vs 24) is the one tuning knob most likely to need a live adjustment — log it.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `StuckDetector` (4u/1s; Mild@1s, Hard@5s); remove duplicates | `brain/src/recover.rs`, `brain/src/nav.rs`, `qbots/src/main.rs` | pending | |
| 2 | T2: `find_best_direction` 7-dir fan + ledge/liquid rules | `brain/src/recover.rs`, `brain/tests/recover.rs` | pending | TRACE_DIST=256 |
| 3 | T3: `Recovery::evaluate` → `RecoveryAction` + goal blacklist | `brain/src/recover.rs`, `brain/src/nav.rs` | pending | |
| 4 | T4: wire into steering pipeline; recorder `recovery` flag | `qbots/src/main.rs`, `brain/src/recorder.rs` | pending | needs Plan 12 |
| 5 | T5: live confirmation + pitfall note | `context/pitfalls.md` | pending | tight-corner map |
