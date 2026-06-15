# Brain (brain) — Tracker

## Overview
- Status: ~86% complete — T1-T6 done & committed; T7 pending.
- Start date: 2026-06-14
- Plan: `06_brain.md`
- Depends on: Plan 05 (world trace + nav graph) and transitively Plan 04 (perception)
- Exit criterion: a single bot navigates, picks up items, engages enemies, scores frags.

## Resume Instructions
1. T1-T6 all committed. Only T7 (live verification) remains.
2. Run the bot against a real server; watch FSM transitions in logs; verify nav, pickups, combat.
3. Tune skill params; record findings in `context/distilled.md`.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: perception / Worldview | `brain/src/perception.rs` | done | classify entities, decay system, query methods; **verified** (3 tests pass) |
| 2 | T2: navigation driver | `brain/src/nav.rs` | done | A* over nav graph + stuck recovery; **verified** (3 tests pass) |
| 3 | T3: move controller wiring | `brain/src/move_ctrl.rs` | done | intent → Usercmd; deg_to_short fix committed |
| 4 | T4: combat / aim / weapons | `brain/src/{combat,aim,weapons}.rs` | done | lead-aim, weapon-select; reviewer feedback addressed |
| 5 | T5: behavior FSM | `brain/src/fsm.rs` | done | Roam/Hunt/Engage/Flee/Pickup; lib.rs wired |
| 6 | T6: skill config | `brain/src/skill.rs` | done | BotSkill + SkillRegistry; 7 tests pass |
| 7 | T7: verify fragging bot | — | pending | multi-minute live run |
