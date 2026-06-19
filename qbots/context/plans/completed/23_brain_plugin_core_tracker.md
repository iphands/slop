# Brain Plugin Core — Tracker

## Overview
- Status: DONE — `trait Brain` seam + `build_brain` factory; binary drives `Box<dyn Brain>`.
- Start date: 2026-06-18
- Contract: **zero behavior change** — `tick` decision body byte-identical (pure
  BrainContext/BrainMap destructure adapter). Static + unit-test verified; live A/B pending a
  reachable server (`noir40.lan` was down this session).

## Resume Instructions
Read `23_brain_plugin_core.md` and Plan 22 (`completed/22_brain_seam_extraction.md`) — the
concrete `Brain::tick` body is the thing being wrapped, untouched. This plan only adds a trait
+ factory and points the binary at a `Box<dyn Brain>`. If interrupted mid-task, check the
Progress table for the last `done` row and re-run `cargo build && cargo clippy && cargo test`.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: seed `brain_notes.md` + append-rule | `context/brain_notes.md` | done | |
| 2 | T2: `brains::core` trait + I/O types | `crates/brain/src/brains/core.rs`, `mod.rs`, `lib.rs` | done | |
| 3 | T3: existing brain implements trait | `crates/brain/src/brain.rs` (+ main.rs call sites) | done | adapter only; body byte-identical |
| 4 | T4: `BrainKind` + `build_brain` | `crates/brain/src/brains/mod.rs` | done | |
| 5 | T5: `bot_task` drives `Box<dyn Brain>` | `crates/qbots/src/main.rs`, `lib.rs` | done | root `Brain`=trait |
| 6 | T6: verify + close | tracker, `SERIES.md`, `brain_notes.md` | done | static+unit green; live deferred (no server) |

## Baseline (fill in before T1, from a pre-refactor run)
- `spawn-to-spawn --map q2dm1`: `# SUMMARY …`
- `spawn-to-weapon rocketlauncher --map q2dm1`: `# SUMMARY …`

## After (fill in at T6)
- `spawn-to-spawn --map q2dm1`: `# SUMMARY …`
- `spawn-to-weapon rocketlauncher --map q2dm1`: `# SUMMARY …`
- `connect-one`: live, no kick? Y/N
