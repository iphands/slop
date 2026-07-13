# Map-Change Survival + Intermission Handling — Tracker

## Overview
- Status: 100% complete (all tasks done; final end-to-end verified 2026-07-13)
- Start date: 2026-07-12
- Live target: cosmo.lan:27910 (rcon via http://cosmo.lan:3000/api/rcon/execute), fraglimit 5

## Live Findings (T4, 2026-07-12)
1. **rcon `map X` is a HARD restart** on this server: `SV_InitGame` → `SV_FinalMessage`
   sends `svc_print "Server restarted" + svc_reconnect`, wipes every slot, and drops
   packets while loading. Bots must do the **full** handshake — and must **re-send
   getchallenge** (~2 s cadence) because the first one is swallowed during the load.
2. **Stale FinalMessage copies kill fast rejoiners**: staggered duplicate copies of the
   final packet arrive AFTER our ~20 ms rejoin, the new netchan accepts the old (huge)
   sequence, and the bot obeys a `svc_disconnect` meant for the dead connection —
   abandoning a live slot. 32/40 bots hit this; abandoned+retry slots overflowed
   maxclients=64 → "Server is full." → strict-mode fleet failure. Fix: bind a **fresh
   local port** on `Active→Connecting` so stale copies are undeliverable.
3. The fraglimit rotation path (`gamemap`) is the SOFT flow (stufftext changing/
   reconnect, netchan + slot kept) — no restart, no stale-packet hazard.
4. **Reliable-channel overflow kicks ("`X overflowed`" + `SZ_GetSpace: overflow` on the
   server console)**: 40 simultaneous rejoins each reliable-multicast skin configstrings
   + join prints to every client; the burst blows the ~1.4 KB per-client reliable buffer
   and `SV_SendDisconnect` (sv_send.c:577) kicks the victims — can hit REAL clients too.
   Mid-pump (cs_connected) victims get a bare 1-byte `svc_disconnect` because the
   "overflowed" broadcast only reaches spawned clients. Fix: deterministic 0–8 s
   name-hash jitter staggers each bot's re-handshake (hard path holds getchallenge;
   soft path defers the reliable "new" via `Conn::rejoin_pending`/`send_new`).

## Resume Instructions
Read Plan 64. T1 is pure `Conn` FSM work (unit-testable offline). T2/T3 are `bot_task`
in `crates/qbots/src/main.rs`. T4 needs the live server: start a competition fleet, rcon
`map q2dm2` mid-run, then let fraglimit 5 rotate the level.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: changing/reconnect stufftext in Conn | `crates/client/src/conn.rs` | done | unit test `map_change_stufftext_rehandshakes_on_live_netchan` |
| 2 | T2: per-map reset + deadline reset | `crates/qbots/src/main.rs` | done | + getchallenge resend, fresh socket on hard reconnect, retryable post-Active errors (live findings 1–2) |
| 3 | T3: intermission freeze + ATTACK vote | `crates/qbots/src/main.rs`, `q2proto/playerstate.rs` | done | |
| 4 | T4: live verification | — | done | run 8 (2026-07-13): full pass, see log below |

## Final Verification (run 8, 2026-07-13)

32-bot competition fleet vs cosmo.lan (fraglimit 5, sv_maplist populated by
qctrl Plan 12's watchdog — comma-joined, all 8 dm maps).

**Phase 1 — hard rcon `map q2dm1`:** 32/32 re-handshakes (fresh socket + 0–8 s
jitter), 37 q2dm1 nav loads (32 + 5 retries), 13 transient kicks ALL
auto-recovered by the supervisor (`had_session` rejoin retry), **0 fatal join
failures**, fleet fighting on the new map within the jitter window.

**Phase 2 — fraglimit rotation (the original crash scenario):** intermission
detected 20:04:18 → bots froze, pressed ATTACK past the 5 s gate → server
rotated q2dm1 → q2dm2 via sv_maplist — **no `maps/.bsp` crash** — all 32 bots
took the soft path (32× "soft map change: sending new", staggered), reloaded
q2dm2 nav (65 total q2dm2 loads incl. initial join), and kept fighting
(49 combat events in the last 60 log lines at check time).

This run also satisfies qctrl Plan 12 T4 step 4 (fraglimit match-end rotation
with a fleet connected — no crash, no game shutdown).

Earlier runs (1–7) and their findings are in the Live Findings section; each
fix landed as its own commit (`4e880de42`…`34087a8de`).
