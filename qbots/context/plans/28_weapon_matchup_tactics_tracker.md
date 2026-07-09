# Plan 28 — Weapon matchup tactics — Tracker

## Overview
- Status: 0% complete
- Start date: —
- Goal: enemy-weapon inference (VWep `modelindex2` → CS_MODELS), per-weapon ideal-range
  positioning replacing `IDEAL_DIST`/`BACKUP_DIST`, matchup-gated engage bias in `main`.

## Resume Instructions
1. Read `28_weapon_matchup_tactics.md`. FIRST verify VWep is live on our server
   (`connect-one` capture: player entities' `modelindex2` + CS_MODELS strings) — T1's
   inference depends on it; record the real wield-model string forms in `pitfalls.md`.
2. Switching already exists (`weapons.rs:206-254`, `combat.rs:143-156`) — do NOT rebuild it;
   this plan is inference + positioning.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: enemy weapon inference | `perception.rs`, `weapons.rs` | pending | `Option<Weapon>`, never guess |
| 2 | T2: per-weapon range bands in `main` | `weapons.rs`, `brains/main.rs` | pending | replaces fixed consts |
| 3 | T3: `matchup_score` + engage bias | `weapons.rs`, `brains/main.rs` | pending | persona-scaled |
| 4 | T4: live A/B vs Plan 45 baseline (kd 0.68) | `brain_notes.md` | pending | 2× 5-min runs |
