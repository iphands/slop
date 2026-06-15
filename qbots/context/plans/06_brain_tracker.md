# Brain (brain) — Tracker

## Overview
- Status: ~14% complete — T1 done & verified; T2-T7 pending.
- Start date: 2026-06-14
- Plan: `06_brain.md`
- Depends on: Plan 05 (world trace + nav graph) and transitively Plan 04 (perception)
- Exit criterion: a single bot navigates, picks up items, engages enemies, scores frags.

## Resume Instructions
1. Confirm Plans 04 + 05 done (snapshots + trace/nav graph). Brain needs both.
2. Reference bot behavior in `vendor/3zb2-zigflag/src/bot/{bot,za,fire}.c` — port algorithms, not APIs.
3. Build perception→nav→move first (T1–T3); combat (T4) and FSM (T5) layer on top.
4. Skill/personality (T6) last — it just scales T4/T5 outputs.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: perception / Worldview | `brain/src/perception.rs` | done | classify entities, decay system, query methods; **verified** (3 tests pass) |
| 2 | T2: navigation driver | `brain/src/nav.rs` | pending | A* + stuck recovery |
| 3 | T3: move controller wiring | `brain/src/move_ctrl.rs` | pending | intent → Usercmd; pmove consts |
| 4 | T4: combat / aim / weapons | `brain/src/{combat,aim,weapons}.rs` | pending | lead-aim, weapon-select |
| 5 | T5: behavior FSM | `brain/src/fsm.rs` | pending | Roam/Hunt/Engage/Flee/Pickup |
| 6 | T6: skill config | `brain/src/skill.rs` | pending | eraser bots.cfg style |
| 7 | T7: verify fragging bot | — | pending | multi-minute run |
