# Connection (client) — Tracker

## Overview
- Status: ~85% — T1–T7 done; **T8 (live verification) pending a reachable server**.
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
| 1 | T1: netchan over UDP | `client/src/netchan.rs` | done | pure port; 6 tests |
| 2 | T2: connect FSM + async `run` | `client/src/conn.rs` | done | FSM handshake tested w/ crafted pkts |
| 3 | T3: userinfo builder | `client/src/userinfo.rs` | done | bot defaults; 2 tests |
| 4 | T4: parse serverdata/CS/stufftext | `client/src/parse.rs` | done | baselines→Unhandled (Plan 04); 4 tests |
| 5 | T5: `begin` → Active | (in conn.rs `on_payload`) | done | begin queued on serverdata; Active set |
| 6 | T6: keepalive heartbeat | (in conn.rs `keepalive`) | done | empty netchan transmit; **clc_move+checksum deferred to Plan 04** |
| 7 | T7: `connect-one` CLI | `qbots/src/main.rs` | done | clap; `qbots connect-one --addr …` |
| 8 | T8: integration vs real server | — | **pending** | needs reachable server; run `connect-one`, check qctrl `status` ≥60s, save gold packet |

## Deferred / Notes
- **T8 is the live milestone** — the handshake logic is FSM-tested but **not yet verified
  against a real server**. Run when a yquake2/q2pro server is reachable.
- **`clc_move` real movement + checksum** (`COM_BlockSequenceCRCByte`, `cl_input.c:787`)
  deferred to Plan 04. For now keep-alive = empty netchan transmit (valid; refreshes
  `last_received`). May need real `clc_move` if a server drops idle-input clients.
- **svc_spawnbaseline / svc_frame** decode (needed to parse past baselines) is Plan 04 T1;
  for Plan 03 we stop at the first `Unhandled` op, which suffices to reach Active.
