# Brain Plugin Core — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-18
- Contract: **zero behavior change** — `spawn-to-spawn`/`spawn-to-weapon` SUMMARY lines and
  `connect-one` live behavior identical to pre-refactor.

## Resume Instructions
Read `23_brain_plugin_core.md` and Plan 22 (`completed/22_brain_seam_extraction.md`) — the
concrete `Brain::tick` body is the thing being wrapped, untouched. This plan only adds a trait
+ factory and points the binary at a `Box<dyn Brain>`. If interrupted mid-task, check the
Progress table for the last `done` row and re-run `cargo build && cargo clippy && cargo test`.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: seed `brain_notes.md` + append-rule | `context/brain_notes.md` | pending | |
| 2 | T2: `brains::core` trait + I/O types | `crates/brain/src/brains/core.rs`, `mod.rs`, `lib.rs` | pending | |
| 3 | T3: existing brain implements trait | `crates/brain/src/brain.rs` | pending | adapter only, no logic change |
| 4 | T4: `BrainKind` + `build_brain` | `crates/brain/src/brains/mod.rs` | pending | |
| 5 | T5: `bot_task` drives `Box<dyn Brain>` | `crates/qbots/src/main.rs` | pending | |
| 6 | T6: verify zero behavior change + close | tracker, `SERIES.md`, `brain_notes.md` | pending | record before/after SUMMARY |

## Baseline (fill in before T1, from a pre-refactor run)
- `spawn-to-spawn --map q2dm1`: `# SUMMARY …`
- `spawn-to-weapon rocketlauncher --map q2dm1`: `# SUMMARY …`

## After (fill in at T6)
- `spawn-to-spawn --map q2dm1`: `# SUMMARY …`
- `spawn-to-weapon rocketlauncher --map q2dm1`: `# SUMMARY …`
- `connect-one`: live, no kick? Y/N
