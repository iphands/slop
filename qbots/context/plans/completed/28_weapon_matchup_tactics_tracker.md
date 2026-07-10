# Plan 28 — Weapon matchup tactics — Tracker

## Overview
- Status: **DONE (2026-07-10)** — T2 (own-weapon range bands) shipped active; T1/T3 (enemy read)
  shipped dormant (server sends no per-weapon VWep); T4 no-regression sanity passed. The
  enemy-inference features can't be exercised until a VWep-per-weapon server; the valuable
  enemy-independent half (T2) is delivered. Moved to `completed/`.
- Start date: 2026-07-10
- Goal: enemy-weapon inference (VWep `modelindex2` → CS_MODELS), per-weapon ideal-range
  positioning replacing `IDEAL_DIST`/`BACKUP_DIST`, matchup-gated engage bias in `main`.

## KEY FINDING (2026-07-10): enemy weapon is NOT on the wire here
Live VWep verification (`QBOTS_P28_DEBUG`): every player entity carries `modelindex2 = 255`
(a sentinel, CS slot 255 empty) — the enemy's weapon is not transmitted by this yquake2 server
(Plan 28 Risk #1 realized; `pitfalls.md`). So T1's inference + T3's matchup gate can't function
live and ship **dormant** (correct + unit-tested, activate on a VWep-per-weapon server). The
valuable, enemy-independent half — **T2 own-weapon range positioning** — is active and is what
Plan 28 effectively delivers.

## Resume Instructions
1. Read `28_weapon_matchup_tactics.md`. FIRST verify VWep is live on our server
   (`connect-one` capture: player entities' `modelindex2` + CS_MODELS strings) — T1's
   inference depends on it; record the real wield-model string forms in `pitfalls.md`.
2. Switching already exists (`weapons.rs:206-254`, `combat.rs:143-156`) — do NOT rebuild it;
   this plan is inference + positioning.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: enemy weapon inference | `perception.rs`, `weapons.rs` | done (dormant) | `from_wield_model` + `held_weapon`; unit-tested; live=None (modelindex2=255). `QBOTS_P28_DEBUG` diagnostic |
| 2 | T2: per-weapon range bands in `main` | `weapons.rs`, `brains/main.rs` | done | `ideal_range`→`RangeBand`; fixed `IDEAL_DIST/BACKUP_DIST` gone; splash backup ≥ min_safe; unit-tested |
| 3 | T3: `matchup_score` + engage bias | `weapons.rs` | primitive done | `matchup_score` pure + unit-tested; engage-bias wiring DEFERRED (needs enemy weapon, unavailable) |
| 4 | T4: live A/B | `brain_notes.md` | sanity-only | full kd A/B is sub-noise at feasible N (noise floor ~0.6 K/D ≫ a positioning tweak); verified by mechanism + no-regression |
