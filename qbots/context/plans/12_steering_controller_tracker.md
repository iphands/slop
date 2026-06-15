# Plan 12 — Steering Controller — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-15
- Goal: bots turn-then-go, pursue look-ahead, never orbit, can circle-strafe

## Before / After metrics (Plan-10 harness)
| Metric | Baseline (pre-12) | After Plan 12 |
|--------|-------------------|---------------|
| orbit frames (spawn-to-spawn) | high | ~0 |
| `wrong_turns` | | |
| `hindered` frames | | |
| elapsed to farthest spawn (s) | | lower |
| circle-strafe visible in `move_yaw` vs `face_delta` | no | yes |
| grounded `max_speed` | | ≤ ~320 (no overspeed) |

## Resume Instructions
1. T1→T2→T3 are the controller building blocks (each unit-tested in isolation); T4 wires them.
2. T4 is the riskiest (touches the live tick) — land T1-T3 + tests first so T4 is a swap.
3. T5 (circle-strafe) depends on T4's `move_from_world_dir`; can land after a T4 baseline run.
4. T6 is cleanup — do it last once T4/T5 prove the new path is sound.
5. `dt` for `change_yaw` must come from observed frame interval (Open Q1), not a constant.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `Steering` + `change_yaw` (turn-rate) | `brain/src/steer.rs`, `brain/src/lib.rs`, `brain/tests/steer.rs` | pending | |
| 2 | T2: `move_from_world_dir` (face-then-go + decomposition) | `brain/src/steer.rs` | pending | |
| 3 | T3: pursue-target + arrive + anti-orbit advance | `brain/src/nav.rs`, `brain/src/steer.rs` | pending | replaces `dist<64` gate |
| 4 | T4: wire `Steering` into `bot_task` | `qbots/src/main.rs` | pending | riskiest |
| 5 | T5: circle-strafe engage movement | `brain/src/steer.rs`, `qbots/src/main.rs` | pending | needs T4 |
| 6 | T6: drop dummy `engage` CombatDecision; single source of truth | `brain/src/fsm.rs`, `qbots/src/main.rs` | pending | cleanup |
