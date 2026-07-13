# Fleet Durability: Active-State Frame-Stall Watchdog — Tracker

## Overview
- Status: 100% complete — closed 2026-07-13
- Start date: 2026-07-13
- Target: full roster survives ≥4 consecutive map cycles (2 min timelimit server) — **met: 9 rotations, 36/36 throughout**

## Resume Instructions
Done. For future durability work: the verification recipe is run the competition roster,
poll OOB `status` every 30 s, and demand player-count == roster at every settled poll;
count-vs-panic arithmetic pins any attrition cause (see pitfalls.md entry).

## Run 1 (pre-T4/T5 binary, 2026-07-13 10:50–11:00) — the smoking gun
Fleet 36/36 through q2dm1→q2dm2, then brain panics started at rotations:
`q3/mod.rs:220:39` ×3 and `main.rs:492:43` ×1 — `index out of bounds: len 6279,
index 8501` (q2dm1→q2dm3). Each panic unwound that bot's supervisor loop → bot gone
forever. Poll: 36 → 35 (q2dm3) → 35 → 34/32 (q2dm5) = **36 − 4 panics = 32 exactly**.
Confirms the user's 16-survivor observation was panic attrition across rotations
(bigger→smaller map transitions), compounding per cycle.

## Run 2 (fixed binary, 2026-07-13 11:01–11:19) — PASS
36-poll window (~18 min), rotation ≈ every 2 min: q2dm2→dm6→dm7→dm8→dm1→dm2→dm3→dm7→dm8→dm1
(**9 rotations**). Player count 36/36 at every settled poll; each rotation showed at most a
one-poll dip (35/34/31, and one 1-player trough mid hard-change at 11:14:42) that recovered
to 36/36 by the next 30 s poll via the supervisor retry path (`reconnecting … backoff`).
**0 panics, 0 frame-stall trips, 0 give-ups, 0 fleet failures.** The stall watchdog never
needed to fire in this window — it remains the safety net for the missed-svc_reconnect hang.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: frame-stall watchdog + `stall_timeout_ms` | `main.rs`, `config.rs` | done | commit 262679d95 |
| 2 | T2: reset reconnect budget on success | `supervisor.rs` | done | commit ffb71086a — time-based (≥60 s session) |
| 3 | T4: reset `roam_idx` in `set_map` (q3/main/xon) | `brain/brains/*` | done | commit 6f13cf044 + regression test |
| 4 | T5: contain brain panics at task boundary | `supervisor.rs` | done | commit 37531802c — JoinError → retryable |
| 5 | T3: live ≥4-map-cycle durability run | — | done | run 2 PASS (9 rotations, 36/36); pitfalls.md entry added |
