# Plan 29 — Engagement: chase / disengage / third-party — Tracker

## Overview
- Status: 0% complete
- Start date: —
- Goal: `main` chases for the kill (extrapolated, nav-pathing, persona/matchup-budgeted
  Hunt), disengages when losing, breaks 1v1s when third-partied.

## Resume Instructions
1. Read `29_engagement_chase_disengage.md`. `Hunt { last_enemy_pos }` exists (`fsm.rs:17`)
   but is a walk-at-point, not a pursuit; q3's `BattleChase` (10s deadline) is the texture
   reference — q3 itself stays untouched (control brain).
2. Enemy health is NOT on the wire — "winning" comes from the T1 `EngageRead` estimator
   (pressure + own health trend + Plan 28 matchup).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `EngageRead` estimator (pure) | `combat.rs`/`engage.rs` | pending | unit-tested |
| 2 | T2: pursuit-capable Hunt + chase budget | `fsm.rs`, `brains/main.rs` | pending | vel-extrapolated goal |
| 3 | T3: third-party break | `brains/main.rs`, `perception.rs` | pending | log `third_party` |
| 4 | T4: live verification + notes | `brain_notes.md` | pending | kills ≥ / deaths ≤ baseline |
