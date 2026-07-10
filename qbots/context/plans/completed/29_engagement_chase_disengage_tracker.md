# Plan 29 — Engagement: chase / disengage / third-party — Tracker

## Overview
- Status: ~85% (2026-07-10) — T1 estimator + T2/T3 break-off logic shipped; T4 sanity-only.
- Start date: 2026-07-10
- Goal: `main` chases for the kill (extrapolated, nav-pathing, persona/matchup-budgeted
  Hunt), disengages when losing, breaks 1v1s when third-partied.

## Scope note (2026-07-10)
Enemy health/weapon are NOT on the wire (Plan 28 finding), so the "winning" read is purely
own-state (`EngageTracker`: pressure = fire-on-target proxy, losing = sustained damage w/o
pressure). Delivered: the **disengage half** — break off a chase when losing (persona
`chase_commit`-scaled) or third-partied (damage while target out of LOS). The **velocity-
extrapolated Hunt goal** (T2's pursuit refinement) is deferred — it needs FSM `Hunt` to carry the
enemy's last velocity (state surgery); the existing Hunt-to-last-pos + the break-off gate deliver
the core "pick and finish fights" behavior. Verification is mechanism + unit tests + no-regression
(kd effect noise-limited, per the harness lesson).

## Resume Instructions
1. Read `29_engagement_chase_disengage.md`. `Hunt { last_enemy_pos }` exists (`fsm.rs:17`)
   but is a walk-at-point, not a pursuit; q3's `BattleChase` (10s deadline) is the texture
   reference — q3 itself stays untouched (control brain).
2. Enemy health is NOT on the wire — "winning" comes from the T1 `EngageRead` estimator
   (pressure + own health trend + Plan 28 matchup).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `EngageRead` estimator (pure) | `engage.rs` | done | `EngageTracker`+`EngageRead` (pressure/losing, own-state); 4 unit tests |
| 2 | T2: chase gate (break losing) | `brains/main.rs` | done (gate) | break-off when losing, persona `chase_commit`-scaled. Vel-extrapolated Hunt goal DEFERRED (FSM state surgery) |
| 3 | T3: third-party break | `brains/main.rs` | done | damage-while-target-out-of-LOS → break to retreat_goal; logged |
| 4 | T4: live verification | `brain_notes.md` | sanity-only | kd noise-limited; mechanism + no-regression |
