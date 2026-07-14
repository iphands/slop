# Weapons-picked-up counter — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-13
- Scope: STAT_PICKUP_STRING-based weapon pickup counter → `wp=` scoreboard column

## Resume Instructions
Plan file: `68_weapon_pickup_counter.md`. T1 (brain helper) then T2 (qbots wiring, single
commit — recorder alone is a dead-code warning). Match case-insensitively (vendor says
`HyperBlaster`); exclude Grenades + Blaster (rationale in plan Decisions).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `is_weapon_pickup_name` | `crates/brain/src/weapons.rs` | pending | |
| 2 | T2: tally + detection + `wp=` column | `crates/qbots/src/{stats,main,supervisor}.rs` | pending | |
