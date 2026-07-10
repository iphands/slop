# Plan 30 — Resource decisions: health & ammo — Tracker

## Overview
- Status: 40% complete — T1 + T2 done (2026-07-09), committed & verified
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
| 3 | T3: known-item goals + Flee→health | `items.rs`, `brains/main.rs` | pending | A*-distance, cap 8 |
| 4 | T4: ammo-aware scoring + re-arm | `weapons.rs`, `items.rs`, `main.rs` | pending | dry weapon scores 0 |
| 5 | T5: live verification + notes | `brain_notes.md` | pending | deaths < 25/5min baseline |
