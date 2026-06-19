# `main` Brain Plugin — Tracker

## Overview
- Status: DONE — concrete brain relocated to `brains/main::MainBrain`; `SentryBrain` proves the
  seam; `build_brain` dispatches both. `main` byte-identical (move+rename only).
- Start date: 2026-06-18
- Contract: `main` behavior byte-identical (git rename, decision body verbatim). Static +
  unit-test verified; live `connect-one`/`spawn-to-*` deferred (server `noir40.lan` down).
- **Scenario migration (Plan 22 T4) is NOT here** — it moves to Plan 26 (`RunTesterBrain`).

## Resume Instructions
Plan 23 must be `done` (trait + factory exist). T1 is a `git mv` of `brain.rs` →
`brains/main.rs` + struct rename `Brain`→`MainBrain`; T2 adds `SentryBrain`. `scenario.rs` is
left alone. If interrupted, the Progress table's last `done` row + `cargo build` show where to
resume.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: relocate concrete → `MainBrain` | `crates/brain/src/brains/main.rs` | done | git rename, no logic change |
| 2 | T2: `SentryBrain` reference plugin | `crates/brain/src/brains/sentry.rs` | done | proves seam runs >1 brain |
| 3 | T3: verify `main` unchanged + brain_notes | tracker, `context/brain_notes.md` | done | static+unit green; live deferred |
| 4 | T4: close Plan 24 | `SERIES.md`, plan, tracker | done | Plan 22 T4 → Plan 26 |

## Verification (T3)
- `connect-one`: live — NOT run (server `noir40.lan` unreachable this session).
- `spawn-to-spawn`/`spawn-to-weapon`: untouched path; relocation is move+rename, body verbatim.
- Static: brain 106 + sentry 2 unit tests green; workspace build/clippy(-D warnings)/fmt clean.
