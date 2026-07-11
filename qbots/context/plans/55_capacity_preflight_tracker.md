# Capacity Preflight — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-11
- Scope: 2 source files. Fail fast before spawning when the roster exceeds free server slots.

## Resume Instructions
T1 = helper + wiring (+ unit test); T2 = live verify + close. Each task: implement →
`cargo fmt` → `cargo clippy -- -D warnings` → `cargo test` → commit `task(TN): …`.
Strict (default) = exit non-zero before spawning; `--loose-botcap` = warn + proceed.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: preflight_capacity + fits_capacity + wiring | `supervisor.rs`, `main.rs` | pending | pub(crate) query_status; 2 call sites |
| 2 | T2: live verify + close | `context/` | pending | over-capacity exits before spawn; move to completed/ |
