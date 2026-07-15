# Roster-file competitions — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-15
- Scope: `--roster <file.yaml>` for competition + ranked roster dump at FINAL

## Resume Instructions
Plan: `69_roster_competitions.md`. Order T1→T6 (T2 needs T1's `matrix_specs`; T5 needs T3's
loader). Key invariant: the matrix CLI path must stay behavior-identical — `matrix_specs`
reproduces today's tags/qports/skins exactly (T1 equivalence test is the contract). Emitter
reuses the `*_code` fns so rosters round-trip. One commit per task, fmt/clippy/test green each.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: GroupSpec + matrix_specs + visibility | `crates/qbots/src/supervisor.rs` | pending | |
| 2 | T2: run_competition consumes Vec<GroupSpec> | `crates/qbots/src/{supervisor,main}.rs` | pending | |
| 3 | T3: roster.rs loader + validation | `crates/qbots/src/roster.rs` | pending | |
| 4 | T4: --roster flag + dispatch | `crates/qbots/src/main.rs` | pending | |
| 5 | T5: emit_ranked_yaml + FINAL dump hook | `crates/qbots/src/{roster,supervisor}.rs` | pending | |
| 6 | T6: docs + SERIES + move to completed/ | `context/` | pending | |
