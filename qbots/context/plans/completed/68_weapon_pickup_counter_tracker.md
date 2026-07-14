# Weapons-picked-up counter — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-07-13 · Closed: 2026-07-13
- Scope: STAT_PICKUP_STRING-based weapon pickup counter → `wp=` scoreboard column

## Resume Instructions
Done — nothing to resume. Live-verified 2026-07-13 on noir.lan:27910 (q2dm1, 6 bots, 2 min):
3 `EVT pickup kind="weapon"` events with correct configstring identities
(`Super Shotgun` ×2, `Grenade Launcher` ×1); per-group attribution matches the FINAL
board exactly (`q3_as wp=2`, `mai_as wp=1`).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `is_weapon_pickup_name` | `crates/brain/src/weapons.rs` | done | Case-insensitive 9-gun match; Grenades/Blaster excluded; HyperBlaster-casing test. `f7d8bc2d6` |
| 2 | T2: tally + detection + `wp=` column | `crates/qbots/src/{stats,main,supervisor}.rs` | done | Stat-transition watcher w/ configstring resolve + map-change reset; wp= on boards + final stats; aggregation tests. `ac957068c` |

## Verification
- [x] T1: unit tests — guns match any-case; Grenades/Blaster/ammo/health names don't
- [x] T2: live EVT count == board wp per group; item names resolve correctly
- [x] fmt/clippy/full-workspace tests green at every commit
