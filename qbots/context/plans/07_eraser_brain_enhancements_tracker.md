# Eraser-Derived Brain Enhancements — Tracker

## Overview
- Status: ~30% — T1 done; T2/T5/T6 partial; T3/T4/T7 pending.
- Depends on: Plan 06 (brain skeleton)
- Exit criterion: a single qbot frags with Eraser-grade aim/dodge/skill — hitscan+projectile aim connects,
  `combat>=4` bots dodge visible rockets/grenades, skill tiers visibly differ, auto-skill drifts, give-up/stuck fire.

## Resume Instructions
1. Read `context/distilled/eraser.md` FIRST — every constant/formula and `bot_*.c:line` ref is there.
2. Plan 06 must be far enough that `crates/brain` exists with perception + a combat stub. This plan *fills in*
   Eraser's numbers; it does not re-architect Plan 06.
3. Plugin-only substitutions: `gi.trace`/`visible`→our `world/` BSP trace; enemy `velocity`→derived from origin
   deltas (low-pass); enemy `health`/`weapon`→not transmitted, use hit-derived estimates; `Pmove` oracle→drop (server runs physics).
4. Keep Eraser's calibrated defaults; only tune per T7.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: combat aim/lead/jitter | `brain/src/{combat,aim}.rs` | done | per-weapon lead (RL/HB/GL/BFG), `(5−acc)/5*2` jitter, ±15° pitch; 10 tests |
| 2 | T2: fire timing + weapon select | `brain/src/weapons.rs` | partial | weapon select reworked in plan 06 (use stringcmd); fire-interval/reaction/lockout pending |
| 3 | T3: danger avoidance | `brain/src/danger.rs` | pending | rocket(>=4)/grenade dodge, `botJumpAvoidEnt` on BSP |
| 4 | T4: skill/personality config | `brain/src/skill.rs` | pending | 7-field, `AdjustRatingsToSkill`, auto-skill |
| 5 | T5: FSM give-up/stuck/engage | `brain/src/{fsm,nav}.rs` | partial | **ideal-distance done** (hold 160u, back up <80u); 2s/4s give-up + RL-retreat pending |
| 6 | T6: fix Eraser gaps | `brain/src/{items,weapons}.rs` | partial | BFG lead dist/400 done (T1); powerup values + camping pending |
| 7 | T7: verify | — | pending | frags + dodge + skill tiers |
