# `main` Brain Plugin — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-18
- Contract: `main` behavior byte-identical; scenarios reproduce Plan 23 T6 "After" numbers.
- Closes: Plan 22 deferred T4 (scenario.rs onto Brain) + retires Plan 15 duplication.

## Resume Instructions
Plan 23 must be `done` (trait + factory exist). T1 is a `git mv` of `brain.rs` →
`brains/main.rs` + struct rename `Brain`→`MainBrain`; T2 adds `SentryBrain`; T3 migrates
`scenario.rs`. If interrupted, the Progress table's last `done` row + `cargo build` show where
to resume. Parity (T4) is the acceptance gate — do not close on drift.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: relocate concrete → `MainBrain` | `crates/brain/src/brains/main.rs` | pending | move + rename, no logic change |
| 2 | T2: `SentryBrain` reference plugin | `crates/brain/src/brains/sentry.rs` | pending | proves seam runs >1 brain |
| 3 | T3: migrate `scenario.rs` | `crates/qbots/src/scenario.rs` | pending | combat-off, pinned goal |
| 4 | T4: scenario parity + brain_notes | tracker, `context/brain_notes.md` | pending | match Plan 23 T6 "After" |
| 5 | T5: close Plan 24 | `SERIES.md`, plan, tracker | pending | mark Plan 22 T4 closed |

## Parity check (T4)
- `spawn-to-spawn --map q2dm1`: before `# SUMMARY …` / after `# SUMMARY …`
- `spawn-to-weapon rocketlauncher --map q2dm1`: before `# SUMMARY …` / after `# SUMMARY …`
