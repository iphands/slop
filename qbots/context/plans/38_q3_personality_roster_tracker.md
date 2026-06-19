# Quake 3 personality roster + tuning — Tracker

## Overview
- Status: 0% complete (pending)
- Start date: 2026-06-19
- Plan: `context/plans/38_q3_personality_roster.md`
- Deliverable: selectable `q3` personality roster (`--q3char`/`--q3chars`) + tuned presets.
- Blocked by: Plan 37 (`Q3Brain`) must be in `completed/` first.

## Resume Instructions
Plan 37 ships one default `q3` personality; this plan turns it into a selectable, tuned roster.
Read `vendor/Quake-III-Arena/.../bots/*.c` for archetype value shapes (distill, don't commit).
Build T1 (selection) → T2 (competition matrix) → T3 (optional observed-inventory) → T4 (live
tuning) → T5 (docs). T4 needs a running q2dm1 server and is iterative. Keep all non-`q3` brains
and Plan 37's held-weapon default behavior unchanged (additive only).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `--q3char` selection + skin/name | `qbots/src/*`, `brains/mod.rs` | pending | default = from_skill (P37 behavior) |
| 2 | T2: `competition --q3chars` matrix | `qbots/src/supervisor.rs` | pending | per-char group + scoreboard |
| 3 | T3: observed-inventory aggression (optional) | `brain/src/observed.rs`, `q3char.rs` | pending | best-owned weapon; flagged |
| 4 | T4: live tuning pass | `context/mode_perf.md` | pending | per-char frag rates; intentional spread |
| 5 | T5: brain_notes + docs | `context/brain_notes.md`, README | pending | dated Plan 38 entry + value sets |

## Verification
- [ ] `cargo build` zero warnings
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo test -p brain` green
- [ ] `cargo fmt` applied
- [ ] `--q3char`/`--q3chars` work; scoreboard separates characters
- [ ] Live: roster fields distinct, balanced characters on q2dm1
- [ ] Non-`q3` brains unchanged
