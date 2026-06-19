# Quake 3 brain plugin (`q3`) — Tracker

## Overview
- Status: 0% complete (pending)
- Start date: 2026-06-19
- Plan: `context/plans/37_q3_brain_plugin.md`
- Deliverable: `Q3Brain` (`BrainKind::Quake3`, `--brain q3`) — Q3 node FSM + aim/fire model.
- Blocked by: Plan 36 (q3char core) must be in `completed/` first.

## Resume Instructions
Read `context/distilled/quake3.md` (§1, §4–7) and the Plan 37 file. Plan 36 provides
`q3char::{Q3Character, bot_aggression, wants_to_retreat, wants_to_chase}` and
`Weapon::power_tier()` — those are the primitives this brain assembles. Build T1→T2 (skeleton +
FSM) so the bot walks, then T3→T5 (enemy select + aim + fire/move), then T6 (wiring), T7 (tests),
T8 (live), T9 (docs). Reuse `Navigator`/`steer`/`recover`/`los` — do **not** fork `MainBrain`.
Each task commits independently. Live T8 needs a running q2dm1 server (`noir40.lan` historically).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: skeleton + node enum + roam SeekLtg | `brains/q3/mod.rs` | pending | walks before combat lands |
| 2 | T2: node FSM transitions | `brains/q3/mod.rs` | pending | aggression-gated retreat/chase; switch-guard ≤50 |
| 3 | T3: Q3 enemy selection | `brains/q3/mod.rs` | pending | alertness range, awareness FOV, LOS |
| 4 | T4: Q3 aim model | `brains/q3/aim.rs` | pending | per-weapon acc, reaction gate, lead, error model |
| 5 | T5: fire-throttle + circle-strafe | `brains/q3/aim.rs`, `move.rs` | pending | duty cycle, self-preservation, jump/crouch dodge |
| 6 | T6: wire BrainKind::Quake3 | `brains/mod.rs`, `qbots/src/*` | pending | --brain q3, config, competition |
| 7 | T7: deterministic tests | `brain/tests/q3_brain.rs` | pending | StubNav transitions + fire gate |
| 8 | T8: live acceptance + A/B vs main | (live) `brain_notes.md` | pending | q2dm1; ≥1 frag/30s, no kicks/panics |
| 9 | T9: brain_notes + docs | `context/brain_notes.md`, README | pending | dated Plan 37 entry |

## Verification
- [ ] `cargo build` zero warnings
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo test -p brain` green (incl. `tests/q3_brain.rs`)
- [ ] `cargo fmt` applied
- [ ] `--help` shows `q3` brain; `--brains main,q3` competition works
- [ ] Live: q3 bot connects + fights on q2dm1, no kicks/panics
- [ ] `main`/`sentry`/`runtester` behavior unchanged
