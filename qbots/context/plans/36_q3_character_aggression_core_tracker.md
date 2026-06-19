# Quake 3 character + aggression core — Tracker

## Overview
- Status: 0% complete (pending)
- Start date: 2026-06-19
- Plan: `context/plans/36_q3_character_aggression_core.md`
- Deliverable: `brain::q3char` (Q3Character + bot_aggression) — pure, unit-tested, no brain yet.

## Resume Instructions
Read `context/distilled/quake3.md` §2–3 and the Plan 36 file. Implement T1→T5 in order;
each task is independently committable. The module is pure (no nav/server) so all tests run
under `cargo test -p brain`. Do **not** touch `MainBrain`/`combat.rs`/`aim.rs` — additive only.
Plan 37 (`Q3Brain`) consumes this module; do not start it until Plan 36 is moved to `completed/`.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `Weapon::power_tier()` | `brain/src/weapons.rs` | pending | Q3 aggression tiers; cite ai_dmq3.c:2199 |
| 2 | T2: `Q3Character` + from_skill + presets | `brain/src/q3char.rs` | pending | named [0,1] traits; grunt/major/sarge/camper |
| 3 | T3: `bot_aggression`/`feeling_bad`/retreat-chase | `brain/src/q3char.rs` | pending | wire-visible inventory only |
| 4 | T4: threshold + preset unit tests | `brain/src/q3char.rs` | pending | rail→press, MG-hurt→flee, boundary |
| 5 | T5: brain_notes entry + docs | `context/brain_notes.md`, `q3char.rs` | pending | PVS deviation + threshold bias rationale |

## Verification
- [ ] `cargo build` zero warnings
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo test -p brain` green
- [ ] `cargo fmt` applied
- [ ] `MainBrain`/`RunTester`/`Sentry` behavior unchanged (additive only)
