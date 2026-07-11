# Ack-on-frame Ping Re-phasing — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-11
- Scope: Re-phase `clc_move` from a free-running 10 Hz timer to frame arrival so the
  server-measured ping drops to ≈RTT. Send rate + movement speed unchanged (dedupe +
  cached msec).

## Resume Instructions
Root cause (vendor-confirmed): Q2 ping = avg over 16 frames of
`(recv time of clc_move acking frame N) − (senttime of frame N)` (`sv_user.c:686-696`,
`sv_main.c:131-164`). Our 10 Hz free-running send adds ~50 ms of self-inflicted phase.
Fix: ack on frame arrival (recv arm detects `conn.frame.serverframe` change), demote the
timer to a 90 ms-gated keepalive. Keep the per-packet `msec` identical so movement is
byte-identical (guardrail against 2× speed).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `SendTiming` primitive | `client/src/send_timing.rs`, `lib.rs` | pending | pure, unit-tested |
| 2 | T2: hybrid ack-on-frame send | `qbots/src/main.rs` | pending | recv-arm send + timer dedupe + `EVT send_timing` |
| 3 | T3: reference loop parity | `client/src/conn.rs`, `qbots/src/scenario.rs` | pending | scenario opted out (baseline) |
| 4 | T4: distill + close | `context/distilled.md`, `pitfalls.md`, `SERIES.md` | pending | move to completed/ |
