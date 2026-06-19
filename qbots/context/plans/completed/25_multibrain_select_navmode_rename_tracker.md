# Multibrain Selection + `--navmode` Rename — Tracker

## Overview
- Status: DONE — brain (`--brain`) and nav backend (`--navmode`) are independent per-bot axes;
  competition varies both; `--mode`/`--modes` renamed to `--navmode`/`--navmodes`.
- Start date: 2026-06-18
- DEVIATION: spawn-to-* got `--navmode` but NOT `--brain` (scenario.rs has no Brain yet → would
  be a no-op). Plan 26 adds the functional spawn-to-* `--brain` with its RuntesterBrain migration.

## Resume Instructions
Plan 24 must be `done` (`BrainKind::{Main,Sentry}` + `build_brain` exist). T1 makes `BrainKind`
a clap `ValueEnum`; T2–T4 add `--brain`/`--brains` selection; T5 is the `--mode`→`--navmode`
rename (flag + prose only, NOT the `NavMode` type). The acceptance gate is the T6 matrix:
`--brain X --navmode Y` works for every combination.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `BrainKind` ValueEnum + `brain_tag` | `crates/brain/src/brains/mod.rs` | done | clap derive dep on brain |
| 2 | T2: `--brain` on connect-one | `crates/qbots/src/main.rs`, `supervisor.rs` | done | spawn-to-* deferred → Plan 26 |
| 3 | T3: `--brain` for `run` + fleet config | `main.rs`, `supervisor.rs`, `config.rs` | done | `[fleet].brain` default `main` |
| 4 | T4: competition `--brains` matrix | `main.rs`, `supervisor.rs` | done | scoreboard grouped (brain,navmode) |
| 5 | T5: `--mode`→`--navmode` rename | `main.rs`, `scenario.rs`, `supervisor.rs`, `skins.rs`, `README.md`, `mode_perf.md` | done | flag+prose only, `NavMode` type kept |
| 6 | T6: verify matrix + close | tracker, `SERIES.md`, `brain_notes.md` | done | static green; live pending server |

## Matrix check (T6)
- `--help`: `--navmode`/`--navmodes` present on all bot cmds, `--mode` gone; `--brain` on
  connect-one/run, `--brains` on competition. Invalid brain/navmode rejected cleanly.
- 18 test binaries green; build + clippy(-D warnings) + fmt clean.
- Live `connect-one --brain sentry --navmode navmesh` / `competition --brains main,sentry
  --navmodes astar,navmesh` runs deferred — server `noir40.lan` unreachable this session.
