# Quake 3 personality roster + tuning — Tracker

## Overview
- Status: 100% complete (done 2026-06-19; T3 observed-inventory deferred)
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
| 1 | T1: `--q3char` selection + skin/name | `qbots/src/*`, `brains/mod.rs` | done | `Q3CharPreset` + `build_brain` arg; per-char skin; `[fleet].q3char` |
| 2 | T2: `competition --q3chars` matrix | `qbots/src/supervisor.rs` | done | per-char group `q3-grunt-astar` + skin + scoreboard |
| 3 | T3: observed-inventory aggression (optional) | `brain/src/observed.rs`, `q3char.rs` | deferred | blaster-floor already competitive; future option (no code) |
| 4 | T4: live tuning pass | `context/mode_perf.md` | done | major 5.00 / sarge 1.25 / camper 1.00 / grunt 0.00; presets stand |
| 5 | T5: brain_notes + docs | `context/brain_notes.md`, README | done | dated Plan 38 entry; README/help `--q3char`/`--q3chars` |

## Verification
- [x] `cargo build` zero warnings
- [x] `cargo clippy -- -D warnings` clean
- [x] `cargo test -p brain` green
- [x] `cargo fmt` applied
- [x] `--q3char`/`--q3chars` work; scoreboard separates characters (`q3-grunt-astar` etc.)
- [x] Live: roster fields distinct, balanced characters on q2dm1 (intentional frag spread)
- [x] Non-`q3` brains unchanged (additive `build_brain` arg defaults `None`)
