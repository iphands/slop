# Ack-on-frame Ping Re-phasing — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-07-11
- Completed: 2026-07-11
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
| 1 | T1: `SendTiming` primitive | `client/src/send_timing.rs`, `lib.rs` | done | pure, 6 unit tests |
| 2 | T2: hybrid ack-on-frame send | `qbots/src/main.rs` | done | **LIVE q2dm1: ping 16ms (was 50–80); `EVT send_timing ema=0.0 max=0.0`; sends ~10/s (no double-send); 30s run, 0 errors/kicks** |
| 3 | T3: reference loop parity | `client/src/conn.rs`, `qbots/src/scenario.rs` | done | conn.rs re-phased; scenario opt-out comment |
| 4 | T4: distill + close | `context/distilled.md`, `pitfalls.md`, `SERIES.md` | done | notes on disk; SERIES row + milestone; moved to completed/ |

## Live verification (T2, 2026-07-11, q2dm1 @ noir.lan)
- `connect-one --name pingtest0`, 30 s. `qbots status` → `0  16ms  pingtest0`.
- `EVT send_timing ema=0.0 max=0.0 sends=10,20,30… late=0` — self-inflicted phase = 0,
  send cadence exactly ~10/s (one ack per 10 Hz server frame → msec/movement unchanged).
- Full brain pipeline ran (weapon requests, targeting, shooting); 0 errors, 0 kicks.
