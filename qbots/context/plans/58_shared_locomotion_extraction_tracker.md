# Shared locomotion extraction — Tracker

## Overview
- Status: 0% complete
- Start date: —
- Deliverable: one shared `brain::locomotion::follow_path` + `hazard::lava_override` + `brain::roam::roam_goal`; four brains delegating; live no-regression matrix per brain.
- Blocks: Plan 60 (`xon` brain must land on shared locomotion, not add a fifth copy).

## Resume Instructions
1. Read `context/plans/RULES.md` in full, then Plan 58.
2. Read `crates/brain/src/brains/q3/mod.rs` `locomote` (the canonical shape) and diff the other three copies against it BEFORE extracting.
3. Task order is strict: T1 module → T2 runtester (scenario-baseline gate) → T3 q3 → T4 main → T5 zb2 → T6 promotions → T7 close. One brain per task; live-verify before moving on.
4. Live checks need a q2 server running the scenario's map (`noir40.lan` historically). If no server: mark the task `blocked`, do NOT claim done.
5. Do NOT homogenize behavior differences between brains — express them as hooks or surface them here as decisions.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `brain::locomotion` + tests | `crates/brain/src/locomotion.rs` | pending | |
| 2 | T2: migrate runtester | `crates/brain/src/brains/runtester.rs` | pending | scenario SUMMARY is the gate |
| 3 | T3: migrate q3 | `crates/brain/src/brains/q3/mod.rs` | pending | |
| 4 | T4: migrate main | `crates/brain/src/brains/main.rs` | pending | kite/flee via `world_dir_override` |
| 5 | T5: migrate zb2 | `crates/brain/src/brains/zb2.rs` | pending | `legs_vs_aim_yaw` hook (Plan 51 R2) |
| 6 | T6: lava_override + roam_goal promotion | `hazard.rs`, `roam.rs` | pending | |
| 7 | T7: brain_notes + close | `context/brain_notes.md`, SERIES | pending | git mv to completed/ |

## Verification

- [ ] Locomotion unit tests green (`cargo test -p brain`)
- [ ] Per-brain live matrix: s2s q2dm1 / swim railgun q2dm1 / ride quad q2dm3 — reached parity vs same-session control
- [ ] No duplicated lava-override or roam-goal blocks remain (grep)
- [ ] Zero warnings, clippy clean, fmt applied, tests green at every commit (Rule A/B)
