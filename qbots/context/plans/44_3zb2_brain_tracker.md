# Plan 44 — 3ZB2-Style Brain Plugin (`zb2`) — Tracker

## Overview
- Status: 0% complete (plan rewritten 2026-07-09; blocked on Plan 46 for T3)
- Start date: —
- Goal: `Zb2Brain` plugin — committed-route following + `Search_NearlyPod` shortcuts +
  mover route-states over the shared traversal executor; competition vs `q3`/`main`.

## Resume Instructions
1. Read `44_3zb2_brain.md` (2026-07-09 rewrite) — the old T3 "port G_FindRouteLink into
   world/src/nav_generator.rs" is DROPPED (file doesn't exist; our graph is richer).
2. Template: Plan 37's `q3` wiring (`crates/brain/src/brains/q3/`, `brains/mod.rs:27-79`).
3. References: `context/distilled/brains/3zb2_brain.md`, `vendor/3zb2-zigflag/src/bot/`.
4. T1/T2 can land before Plan 46; T3 needs the shared traversal executor.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: skeleton + `BrainKind::Zb2` wiring | `brains/zb2.rs`, `brains/mod.rs`, CLI | pending | mirror Plan 37 |
| 2 | T2: committed route + `Search_NearlyPod` skip | `brains/zb2.rs` | pending | unit-test the skip |
| 3 | T3: mover states over traversal executor | `brains/zb2.rs` | pending | blocked on Plan 46 |
| 4 | T4: weapon-run item bias | `brains/zb2.rs` | pending | reuse `best_item_goal_weighted` |
| 5 | T5: live proof + competition + notes | `mode_perf.md`, `brain_notes.md` | pending | 2× 5-min runs |
