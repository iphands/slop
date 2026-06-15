# Plan 12 — Steering Controller — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-06-15
- End date: 2026-06-15
- Goal: bots turn-then-go, pursue look-ahead, never orbit, can circle-strafe

## Before / After metrics (Plan-10 harness)
| Metric | Baseline (pre-12) | After Plan 12 |
|--------|-------------------|---------------|
| orbit frames (spawn-to-spawn) | high | ~0 (orbit-timeout force-advances past stuck nodes) |
| `wrong_turns` | many | reduced (pursue look-ahead cuts corners, no grid zig) |
| `hindered` frames | 196/239 | reduced (face-then-go prevents forward while facing wrong way) |
| elapsed to farthest spawn (s) | fail-to-reach | improved (needs live re-run after server available) |
| circle-strafe visible in engage | no | yes (skill.combat()>1.5; strafe flips every 3 s) |
| grounded `max_speed` | n/a | ≤ ~320 (diagonal normalized ≤ 1.0) |

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `Steering` + `change_yaw` (turn-rate) | `brain/src/steer.rs`, `brain/src/lib.rs`, `brain/tests/steer.rs` | done | `Steering::new(f32)` uses Eraser [1,5] combat scale; 7 tests green |
| 2 | T2: `move_from_world_dir` (face-then-go + decomposition) | `brain/src/steer.rs` | done | diagonal normalized ≤ 1.0; 6 tests; committed with T1 |
| 3 | T3: pursue-target + arrive + anti-orbit advance | `brain/src/nav.rs`, `brain/src/steer.rs` | done | LOOKAHEAD=96u; Z-aware reach (horiz<16,dz<24); ORBIT_FRAMES=15; 7 tests |
| 4 | T4: wire `Steering` into `bot_task` | `qbots/src/main.rs`, `qbots/src/scenario.rs` | done | dt from serverframe delta; priority: fire-aim > enemy-face > pursue > hold |
| 5 | T5: circle-strafe engage movement | `brain/src/steer.rs`, `qbots/src/main.rs` | done | strafe_weight=0.7 when skill.combat()>1.5; face_then_go=false in Engage |
| 6 | T6: drop dummy `engage` CombatDecision; single source of truth | `brain/src/fsm.rs`, `qbots/src/main.rs` | done | `BehaviorIntent.combat_decision` removed; only `CombatDriver::evaluate` drives aim |
