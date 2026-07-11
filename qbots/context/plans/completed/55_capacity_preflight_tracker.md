# Capacity Preflight — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-07-11
- Completed: 2026-07-11
- Scope: 2 source files. Fail fast before spawning when the roster exceeds free server slots.

## Resume Instructions
Done. Live-verified on a 55/64 server: strict 24-bot run exited 1 pre-spawn; loose warned +
proceeded; 1-bot run passed the gate. Backstopped by Plan 53 at join time.

Code scheme: strict (default) = exit non-zero before spawning; `--loose-botcap` = warn +
proceed; unknown capacity (query fail / no maxclients) = warn + proceed.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: preflight_capacity + fits_capacity + wiring | `supervisor.rs`, `main.rs` | done | pub(crate) query_status; 2 call sites; unit test |
| 2 | T2: live verify + close | `context/` | done | strict exits pre-spawn (0 spawns); loose proceeds; moved to completed/ |
