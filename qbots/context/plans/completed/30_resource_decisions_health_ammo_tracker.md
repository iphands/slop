# Plan 30 — Resource decisions: health & ammo — Tracker

## Overview
- Status: **DONE (2026-07-10)** — T1–T5 complete; roam patrol reverted (regression), Flee→health
  bounded (≤900u A*). Live A/B **inconclusive due to variance** (q3 control kd swung 1.00→2.60 with
  identical code across runs) — combat tuning deferred to the Plan 47 multi-run harness. Kept the
  behavior because it's principled + north-star-aligned + conservatively bounded.
- Start date: 2026-07-09
- Goal: map-known item table in `BrainMap`, per-bot taken/respawn memory, hurt→nearest
  reachable health (A*-distance), ammo-aware weapon scoring + re-arm goals.

## Resume Instructions
1. Read `30_resource_decisions_health_ammo.md`. Item seeking today is PVS-blind
   (`items.rs:45-117` iterates `view.items()` only) — that's the core gap.
2. Reuse: `bsp.find_class` (`world/bsp.rs:238`), `item_classname` aliases
   (`qbots/scenario.rs:63` — move to a shared home), `BrainMap` (`brains/core.rs:44`),
   `held_ammo()` (`perception.rs:90`).
3. Respawn timers: cite `vendor/yquake2/src/game/g_items.c` lines next to the constants.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `BrainMap.items` static table | `perception.rs`, `items.rs`, `core.rs`, `supervisor.rs`, wiring | done | `classify_item_classname` + `build_map_items`; `MapItem{class,origin,nav_node}`; MainBrain stores, q3 ignores. Live q2dm3: 52 spawns |
| 2 | T2: `ItemMemory` + respawn model | `items.rs` | done | per-bot, PVS-honest (500u trust range); observe/available; g_items.c respawn times; 3 unit tests |
| 3 | T3: known-item goals + Flee→health | `brains/main.rs` | done | `nearest_reachable_item` (A*-dist, cap 8); Flee→known health/armor; roam patrols nearest known item (cadence-cached). 2 commits (Flee + roam, separable) |
| 4 | T4: ammo-aware scoring + re-arm | `weapons.rs`, `combat.rs`, `q3` | done | dry held weapon (0 ammo) → Blaster fallback; q3 opts out (`i32::MAX`); re-arm via T3 roam patrol. +1 test |
| 5 | T5: live verification + notes | `brain_notes.md` | done | q2dm3 + q2dm1 A/B run; **inconclusive (variance)**: q3 control kd 1.00→0.86→2.60 same code. Patrol reverted (clear regression); Flee→health bounded. brain_notes + pitfalls appended. Rigorous measurement = Plan 47 harness |
