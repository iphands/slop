# Brain (brain) — Tracker

## Overview
- Status: 100% complete — T1-T7 done & committed. Exit criterion met.
- Start date: 2026-06-14
- Plan: `06_brain.md`
- Depends on: Plan 05 (world trace + nav graph) and transitively Plan 04 (perception)
- Exit criterion: a single bot navigates, picks up items, engages enemies, scores frags. ✓ **VERIFIED 2026-06-15** — bot scored a frag at 33.8s live (`*** FRAG *** frags=1`).

## Resume Instructions
1. Plan complete. T7 live-verified against `noir.lan:27910`.
2. Combat-quality polish (keep-distance, projectile dodge, lead aim) is **Plan 07** scope.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: perception / Worldview | `brain/src/perception.rs` | done | classify entities, decay, queries, +STAT_FRAGS |
| 2 | T2: navigation driver | `brain/src/nav.rs` | done | A* + stuck recovery (+force_replan) |
| 3 | T3: move controller wiring | `brain/src/move_ctrl.rs` | done | intent→Usercmd; **delta_angles fix** |
| 4 | T4: combat / aim / weapons | `brain/src/{combat,aim,weapons}.rs` | done | real Q2 weapons; `use <name>` stringcmd switching |
| 5 | T5: behavior FSM | `brain/src/fsm.rs` | done | Roam/Hunt/Engage/Flee/Pickup |
| 6 | T6: skill config | `brain/src/skill.rs` | done | BotSkill + SkillRegistry |
| 7 | T7: verify fragging bot | — | done | **frag scored live 2026-06-15** |

## T7 Live-Verification Findings (2026-06-15)
- **delta_angles bug** (root cause of "bots run into walls away from target"):
  server adds `pmove.delta_angles` to `usercmd.angles`; we ignored it → constant
  spawn-yaw rotation on aim + movement. Fixed in `move_ctrl.rs`; recorded in
  `context/pitfalls.md`.
- **Stuck recovery**: was jump-only → bot wedged ~34s. Now back-off + jump +
  force_replan → escapes within one cycle.
- **Weapon switching**: Q2 ignores `usercmd.impulse`; switching is `use <name>`
  stringcmd. Reworked `weapons.rs` (correct 10 Q2 weapons + names) + `combat.rs`
  (optimistic held-weapon model). Bot picked up + fired the Super Shotgun live.
- Verified: full-map navigation, item pickup (SSG), combat engagement, **1 frag**.
- Deferred to Plan 07: keep-optimal-distance combat (bot still charges point-blank
  and loses duels), projectile dodge, lead-aim refinement.
