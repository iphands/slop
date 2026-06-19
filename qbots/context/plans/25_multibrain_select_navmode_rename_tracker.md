# Multibrain Selection + `--navmode` Rename — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-18
- Outcome: brain (`--brain`) and nav backend (`--navmode`) are independent per-bot axes;
  competition can vary both; `--mode`/`--modes` renamed to `--navmode`/`--navmodes`.

## Resume Instructions
Plan 24 must be `done` (`BrainKind::{Main,Sentry}` + `build_brain` exist). T1 makes `BrainKind`
a clap `ValueEnum`; T2–T4 add `--brain`/`--brains` selection; T5 is the `--mode`→`--navmode`
rename (flag + prose only, NOT the `NavMode` type). The acceptance gate is the T6 matrix:
`--brain X --navmode Y` works for every combination.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `BrainKind` ValueEnum + `brain_tag` | `crates/brain/src/brains/mod.rs` | pending | |
| 2 | T2: `--brain` on single-bot + scenario | `crates/qbots/src/main.rs` | pending | independent of `--navmode` |
| 3 | T3: `--brain` for `run` + fleet config | `main.rs`, `supervisor.rs`, config | pending | `[fleet].brain` default `main` |
| 4 | T4: competition `--brains` matrix | `main.rs`, `supervisor.rs` | pending | scoreboard grouped (brain,navmode) |
| 5 | T5: `--mode`→`--navmode` rename | `main.rs`, `scenario.rs`, `supervisor.rs`, `README.md`, `mode_perf.md` | pending | flag+prose only, keep `NavMode` type |
| 6 | T6: verify matrix + close | tracker, `SERIES.md`, `brain_notes.md` | pending | |

## Matrix check (T6)
- `connect-one --brain sentry --navmode navmesh`: runs? Y/N
- `competition --brains main,sentry --navmodes astar,navmesh --count 2`: scoreboard groups by (brain,navmode)? Y/N
- `run --help` shows `--navmode` + `--brain`, no `--mode`? Y/N
