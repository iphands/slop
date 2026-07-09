# Plan 30 — Resource decisions: health & ammo — Tracker

## Overview
- Status: 0% complete
- Start date: —
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
| 1 | T1: `BrainMap.items` static table | `world/build.rs`, `brains/core.rs`, wiring | pending | alias helper shared |
| 2 | T2: `ItemMemory` + respawn model | `items.rs` | pending | per-bot, PVS-honest |
| 3 | T3: known-item goals + Flee→health | `items.rs`, `brains/main.rs` | pending | A*-distance, cap 8 |
| 4 | T4: ammo-aware scoring + re-arm | `weapons.rs`, `items.rs`, `main.rs` | pending | dry weapon scores 0 |
| 5 | T5: live verification + notes | `brain_notes.md` | pending | deaths < 25/5min baseline |
