# `main` Brain Plugin — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-18
- Contract: `main` behavior byte-identical; `connect-one` runs live; untouched scenarios still
  reproduce Plan 23 T6 numbers (no-collateral check).
- **Scenario migration (Plan 22 T4) is NOT here** — it moves to Plan 26 (`RuntesterBrain`).

## Resume Instructions
Plan 23 must be `done` (trait + factory exist). T1 is a `git mv` of `brain.rs` →
`brains/main.rs` + struct rename `Brain`→`MainBrain`; T2 adds `SentryBrain`. `scenario.rs` is
left alone. If interrupted, the Progress table's last `done` row + `cargo build` show where to
resume.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: relocate concrete → `MainBrain` | `crates/brain/src/brains/main.rs` | pending | move + rename, no logic change |
| 2 | T2: `SentryBrain` reference plugin | `crates/brain/src/brains/sentry.rs` | pending | proves seam runs >1 brain |
| 3 | T3: verify `main` unchanged + brain_notes | tracker, `context/brain_notes.md` | pending | connect-one live; scenarios untouched |
| 4 | T4: close Plan 24 | `SERIES.md`, plan, tracker | pending | Plan 22 T4 → Plan 26 |

## Verification (T3)
- `connect-one`: live, no kick? Y/N
- `spawn-to-spawn --map q2dm1` (untouched path): matches Plan 23 T6? Y/N
- `spawn-to-weapon rocketlauncher --map q2dm1` (untouched path): matches Plan 23 T6? Y/N
