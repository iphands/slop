# Eraser-Derived Brain Enhancements — Tracker

## Overview
- Status: 100% — T1-T6 done; T7 verified live (6 frags in 30s, 3-bot fleet). RL-retreat deferred (enemy weapon not visible on the wire).
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
| 2 | T2: fire timing + weapon select | `brain/src/{combat,weapons}.rs` | done | per-weapon fire_interval_secs, reaction delay, 0.9s switch lockout; 5 fire-gate tests |
| 3 | T3: danger avoidance | `brain/src/danger.rs` | done | rocket(>=4)/grenade dodge, perpendicular strafe+jump; 5 tests |
| 4 | T4: skill/personality config | `brain/src/skill.rs` | done | Ratings + AdjustRatingsToSkill + on_kill/on_death auto-skill + quad_freak/camper; 3 tests |
| 5 | T5: FSM give-up/stuck/engage | `brain/src/{fsm,nav}.rs` | done | ideal-distance + 80-tick goal give-up watchdog; **RL-retreat deferred** (enemy weapon not on wire) |
| 6 | T6: fix Eraser gaps | `brain/src/{items,weapons}.rs` | done | BFG lead dist/400; explicit powerup item values + best_item_goal; first-cut camping; 3 tests |
| 7 | T7: verify | — | done | **3-bot fleet, 30s: 6 frags, 46 shots, 21 stuck recoveries, nav shared, no kicks** |
