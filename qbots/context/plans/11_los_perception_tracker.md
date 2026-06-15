# Plan 11 — Honest LOS Perception — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-15
- Goal: bots only target/fire-at/chase/navigate-to enemies they can actually see

## Before / After metrics (Plan-10 harness, same map)
| Metric | Baseline (pre-11) | After Plan 11 |
|--------|-------------------|---------------|
| `phantom_target` frames (spawn-to-weapon) | | ~0 |
| `bumps` (spawn-to-weapon) | | |
| bots grinding into walls at walled enemies | yes | gone |

## Resume Instructions
1. T1 (los helper) and T2 (nearest_visible_enemy) land first; T3 builds on T2's target tracking.
2. T2 needs `Arc<CollisionModel>` in the tick — confirm `MapNav` exposes it (Open Q1); add it
   next to `Arc<NavGraph>` if missing.
3. T4's `phantom_target` recorder flag is the proof artifact — wire it even if small.
4. Re-run Plan-10 scenarios to fill the Before/After table; that's the done-criterion.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `los.rs` — `has_los` / `has_los_player` / `eye_origin` + tests | `brain/src/los.rs`, `brain/src/lib.rs`, `brain/tests/los.rs` | pending | |
| 2 | T2: `nearest_visible_enemy` + wire combat/FSM callers | `brain/src/perception.rs`, `brain/src/combat.rs`, `brain/src/fsm.rs` | pending | signature: pass `&LosChecker` |
| 3 | T2b: ensure `Arc<CollisionModel>` available in tick | `qbots/src/supervisor.rs` / `MapNav` | pending | Open Q1 |
| 4 | T3: sight hysteresis (`SIGHT_GRACE_FRAMES=2`) | `brain/src/combat.rs` | pending | |
| 5 | T4: nav-to-enemy only on LOS + `phantom_target` recorder flag | `qbots/src/main.rs`, `brain/src/recorder.rs` | pending | |
| 6 | T5: live before/after + pitfalls/distilled notes | `context/pitfalls.md`, `context/distilled.md` | pending | fill Before/After table |
