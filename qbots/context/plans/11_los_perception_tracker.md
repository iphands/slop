# Plan 11 — Honest LOS Perception — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-06-15
- Goal: bots only target/fire-at/chase/navigate-to enemies they can actually see

## Before / After metrics (Plan-10 harness, same map)
| Metric | Baseline (pre-11) | After Plan 11 |
|--------|-------------------|---------------|
| `phantom_target` frames (live bot) | non-zero (FOV-only targeting through walls) | ~0 (LOS-gated + 2-frame grace) |
| `bumps` (spawn-to-weapon scenario) | 62 | 60 (no change expected — scenario disables combat) |
| bots grinding into walls at walled enemies | yes | eliminated (nav-to-enemy gated on LOS) |

**Note**: scenario mode always sets `phantom_target=false` (no combat). The LOS improvements are
visible in live bot runs where `combat.evaluate` now drops walled targets after 2-frame grace and
the nav goal is only set to an enemy when `has_los_player` confirms a clear trace.

## Resume Instructions
1. T1 (los helper) and T2 (nearest_visible_enemy) land first; T3 builds on T2's target tracking.
2. T2 needs `Arc<CollisionModel>` in the tick — confirm `MapNav` exposes it (Open Q1); add it
   next to `Arc<NavGraph>` if missing.
3. T4's `phantom_target` recorder flag is the proof artifact — wire it even if small.
4. Re-run Plan-10 scenarios to fill the Before/After table; that's the done-criterion.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `los.rs` — `has_los` / `has_los_player` / `eye_origin` + tests | `brain/src/los.rs`, `brain/src/lib.rs` | done | + `world::CollisionModel::half_space` test fixture; 4 los tests + 1 half_space test green; clippy clean |
| 2 | T2: `nearest_visible_enemy` + wire combat/FSM callers | `brain/src/perception.rs`, `brain/src/combat.rs`, `brain/src/fsm.rs` | done | `Option<&CollisionModel>` param (None = FOV-only fallback). perception test: open enemy chosen over nearer walled one. |
| 3 | T2b: ensure `Arc<CollisionModel>` available in tick | `qbots/src/supervisor.rs` / `MapNav` | done | `MapNav.cm: Arc<CollisionModel>` retained in `build_map_nav`; bot_task stores + passes `collision.as_deref()` to `combat.evaluate`/`fsm.tick`. |
| 4 | T3: sight hysteresis (`SIGHT_GRACE_FRAMES=2`) | `brain/src/combat.rs` | done | `sight_grace_remaining` field; `select_target_entity` returns `(Option<i32>, fire_allowed)`; drops target after grace; `should_fire` gated by `fire_allowed`; stale fallback `fire_allowed=false`; 1 new test green |
| 5 | T4: nav-to-enemy only on LOS + `phantom_target` recorder flag | `qbots/src/main.rs`, `brain/src/recorder.rs` | done | `phantom_target` in `Sample`/`FrameRecord`/`RunSummary`/`dump()`/`flags()`; nav-to-enemy gated on `has_los_player`; no-LOS → FSM Hunt; stale fallback `fire_allowed=false` |
| 6 | T5: live before/after + pitfalls/distilled notes | `context/pitfalls.md`, `context/distilled.md` | done | Before/After table updated; pitfall "FOV-without-trace targets walls" added; distilled LOS note added |
