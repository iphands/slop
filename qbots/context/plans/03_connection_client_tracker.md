# Connection (client) — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-06-14
- Plan: `03_connection_client.md`
- Depends on: Plan 02 (`q2proto` must round-trip before we trust the wire)
- Exit criterion: `qbots connect-one …` makes a bot appear in server `status` (via qctrl RCON) and hold ≥ 60 s.

## Resume Instructions
1. Confirm Plan 02 is done: `cargo test -p q2proto` green.
2. Have a local/known server reachable + qctrl able to RCON `status` (needed for T8).
3. Follow the **spawn sequence** (9 steps) in the plan; the FSM task (T2) is the spine.
4. Any wire surprise during T8 → record in `context/pitfalls.md` before fixing.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: netchan over UDP | `client/src/netchan.rs` | pending | tokio + q2proto dep |
| 2 | T2: connect FSM | `client/src/{conn,state}.rs` | pending | 9-step handshake |
| 3 | T3: userinfo builder | `client/src/userinfo.rs` | pending | name/skin/rate/… |
| 4 | T4: parse serverdata/CS/baseline | `client/src/parse.rs` | pending | `cl_parse.c:887` |
| 5 | T5: `begin` → Active | `client/src/spawn.rs` | pending | `cl_download.c:531` |
| 6 | T6: clc_move heartbeat | `client/src/loop.rs` | pending | cap by `rate` |
| 7 | T7: `connect-one` CLI | `qbots/src/main.rs` | pending | clap harness |
| 8 | T8: integration vs real server | — | pending | status holds; save gold packet |
