# sv_maplist Resilience + Empty-Map Guards — Tracker

## Overview
- Status: 100% complete (4/4) — T4 executed live 2026-10-03; one open anomaly (Follow-ups)
- Start date: 2026-07-12
- Incident reference: server died on `ERROR: Couldn't load maps/.bsp` (2026-07-12)
  when a qbots fleet ended a fraglimit match while `sv_maplist` was empty.
  Full forensics: `../qbots/context/plans/64_map_change_survival_tracker.md`.
- Live target: Q2 server on **noir.lan:27910** (`config.yaml`); the qctrl API runs on
  cosmo at `localhost:3000`. (This line said "server on cosmo" until T4 — wrong since the
  server moved to noir.)

## Resume Instructions
All four tasks are closed; next is `/hands-plan close 12` (carry the Follow-ups).
Read Plan 12 in full — the Context section carries the incident forensics and the
design constraints (check-then-push, never push empty, never push on unparseable
reply). T1/T2 are pure-Rust in `crates/api`; T3 is frontend (vitest); T4 needs the
live server + a qbots fleet (coordinate with the operator — they manage API
restarts). Every helper is specified with a code sketch and an enumerated test
table in the plan; implement to those tables.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: sv_maplist drift re-sync loop | `crates/api/src/main.rs` | done | 60 s check-then-push (`spawn_sv_maplist_watchdog`); 9 unit tests green; commit `15234be02` |
| 2 | T2: rcon_execute + rotation-name guards | `crates/api/src/main.rs` | done | `validate_rcon_command` + `valid_map_name`; 400 on empty map/gamemap and on bogus rotation names; 5 unit tests green; commit `605d43dbb` |
| 3 | T3: frontend empty/unknown-map guards | `frontend/src/lib/applyLogic.ts` + components | done | shared `isKnownMap`/`UNKNOWN_MAPS`; 7 vitest cases written; commit `26c6fabbc` |
| 4 | T4: live regression protocol | — | done | 2026-10-03 vs noir, 4-bot fleet. Steps 2–4 pass (drift restored ≤60 s; guards 400; fraglimit end rotated q2dm2→q2dm3 via `sv_maplist`, no crash). Step 1 **anomaly**: `sv_maplist` read `q2dm1` 74 s after API start (open, see Follow-ups). Step 2 as written `invalid` (`set sv_maplist ""` can't be sent over rcon). Step 5 not run live. See Live Verification Log |

## Notes / Deviations

- **The original `set sv_maplist "…"` command never worked at all** (found when the
  watchdog went live, 2026-07-13; fixed in `0ea97f490`). `SVC_RemoteCommand`
  (`vendor/yquake2/src/server/sv_conless.c`) tokenizes the rcon packet and then
  rebuilds the command by re-joining `argv` with spaces — the quotes are gone before
  `Cmd_ExecuteString` ever sees it. So `set sv_maplist "q2dm1 q2dm2"` arrived as
  `set sv_maplist q2dm1 q2dm2` (10 args), the server replied
  `usage: set <variable> <value> [u / s]`, and the cvar stayed empty. **A quoted value
  with spaces cannot survive rcon.** Plan 12's Key Facts asserted this command worked;
  that was wrong, and it means `spawn_sv_maplist_sync` has been a no-op since it was
  written — the server's `sv_maplist` was empty the whole time, which is exactly why
  the crash happened.
  Fix: comma-join, unquoted (`set sv_maplist q2dm1,q2dm2,…`) — `EndDMLevel` tokenizes
  on `" ,\n\r"` (`g_main.c`), and a comma-joined value is a single argv token. Drift is
  now compared map-by-map, so a space-separated value set from the server console is
  not treated as drift (otherwise the watchdog would re-push over a working list every
  minute).
  Verified live against noir: quoted form → usage error, cvar unchanged; comma form →
  all 8 maps assigned.

- **T1**: implemented as `spawn_sv_maplist_watchdog(state)` (a named fn) rather than
  an inline block in `main()`, so it reads like the existing `spawn_sv_maplist_sync`.
  Design constraints from the plan all hold: 60 s interval, never push an empty
  queue, never push on an unparseable reply, not gated on `rotation_enabled`.
- **T2 / Open Question 1**: `frontend/src/lib/api.ts:executeRcon` throws a generic
  error on any `!res.ok`, so a bare `StatusCode::BAD_REQUEST` (no JSON body) keeps
  the UI's error path working. The rejection is also broadcast on the log stream as
  an `ERROR` line, so the in-app console says why nothing happened.
- **T3**: `npm run test` is **green — 30 passed / 4 files**, including the 7 new
  `buildApplyCommands` cases. `npm run lint` and `npm run build` are clean too.
  Getting there needed a frontend toolchain repair (commit `493d7c9db`), all of it
  pre-existing breakage rather than anything Plan 12 introduced:
  - **Use node 22** (`frontend/.nvmrc`, `just fe-node-check`). The system node 24.14
    on this host SIGSEGVs inside vite: `npm ci`, `vite build` and `vitest` all die
    with no output at all. Node 22 runs all three clean. This is the trap that makes
    the whole toolchain look broken — the crash prints nothing.
  - `vitest` was pinned at 2.x, which supports vite ≤5; against vite 8 it collected
    zero suites ("No test suite found"). Now on 4.x.
  - vitest and eslint both crawled into `node_modules.gentoo` — their built-in
    `node_modules` ignores don't match the justfile's per-env tree name. vitest ran
    zod's 185 locale suites; eslint died on a config inside a dependency. Both
    configs now ignore `node_modules*`.
  - Two stale suites had never been runnable and were fixed: `Rotation.test.tsx`
    was missing the `NotificationsProvider` the page now requires, and
    `useRotationTimer` asserted the countdown is `< 1200` when it starts at exactly
    1200.
- Pre-existing `dead_code` warning on `LogStream::get_history` (`crates/api/src/logs.rs:83`)
  is untouched — not introduced here, and out of this plan's scope.
- **T4 step 2 — `invalid` as written.** `rcon set sv_maplist ""` cannot blank the cvar:
  `SVC_RemoteCommand` rebuilds the command from argv, the empty quoted token becomes
  nothing, `set` sees 2 args and replies `usage: set <variable> <value> [u / s]` (seen
  live, cvar unchanged). Same root cause as the comma-join note above. Substituted
  `set sv_maplist ,` (zero maps to both the game and `sv_maplist_maps`) and
  `set sv_maplist q2dm1` (the exact state step 1 found).
- **T4 step 4 — fraglimit 5 → fraglimit = current top score.** The `roster-lite` fleet
  frags slowly: top score 3 after 10 min, so fraglimit 5 never triggered (run 1, restored
  to 0, no rotation). Run 2 set fraglimit to the current top (3): `CheckDMRules` ends the
  match on the next frame through the same `EndDMLevel` path, and scores reset on the new
  map so it cannot cascade. A timelimit end would NOT test this plan: the rotator preempts
  at `timelimit - 5 s` and fires its own `map`, bypassing `sv_maplist`.
- **T4 step 5 — not run live.** Needs the server down or mid-change plus a browser. The
  guarded logic (`isKnownMap`, `buildApplyCommands` skipping the implicit restart) is
  covered by the T3 vitest cases, green again in this task's gate.
- **T4 verification limits.** No live server console is reachable from cosmo (the only
  `qconsole.log` on the noir mount is from 2023), so "no `Rcon from` line" and "no
  `maps/.bsp`" are inferred: the rejects return 400 before `rcon_client.execute` is called,
  the server stayed on the same map with all players across them, and after the match end
  it was online on the predicted map with all 4 bots back. The API's own stdout (the
  `Synced sv_maplist` / `drifted` lines) was in the operator's terminal and not captured;
  restores were observed by reading the cvar back over rcon.
- **Gate incident during T4 (2026-10-03 14:05, restored 2026-10-04 06:06).** `just
  fe-test` runs `npm run testall` = lint + build + `node e2e-test.js`, and that script
  drives the *live* API at `cosmo.lan:3000` — which the operator's API was now serving. In
  11 s it sent `dmflags 0` (never restored), `timelimit 0/900`, `fraglimit 50/0`, three
  `map` restarts with the fleet connected, and removed the pre-existing favorite `q2dm1`
  (its "cleanup" deletes `maps[0]` even when the add was a no-op). Restored by hand:
  `dmflags 17424`, favorites rebuilt through the API so `favorites.json` matches HEAD. The
  `_nm` recipe also dropped a self-referencing `node_modules.gentoo` symlink inside
  `frontend/node_modules` (a real directory here, not the per-env symlink); removed. The
  gate was re-run as `cargo fmt --check`, clippy `-D warnings`, `cargo test`, and vitest
  only (`npm run test`). Pitfall written to `context/pitfalls.md`.

## Follow-ups

| # | Item | Why | Where |
|---|------|-----|-------|
| 1 | **Step-1 anomaly: `sv_maplist` was `q2dm1` 74 s after API start**, after the startup push and the ~13:44:46 watchdog tick should both have set the full list | Unexplained. The same watchdog repaired the identical `q2dm1` state in 60 s later (step 2c). Leads: the fleet connected at 13:44:47, the same second as that tick — a lost/garbled reply is logged only at `debug` (`sv_maplist check skipped`) or parses as `None` (no push, by design); and `push_sv_maplist` maps *any* reply to `Ok`, so a push the server rejected still logs `Synced sv_maplist`. yquake2 has no rcon rate limiter, so throttling is not the cause. Next: capture the API's stdout from a cold start (`RUST_LOG=debug`), and consider verifying the push by reading the cvar back (the rcon pitfall's own advice) | `crates/api/src/main.rs` (`push_sv_maplist`, `spawn_sv_maplist_watchdog`) |
| 2 | **`e2e-test.js` mutates a live server and is wired into `testall`/`just fe-test`** | Hard-coded `cosmo.lan:3000`; leaves `dmflags 0`; deletes a pre-existing favorite. Make it opt-in (env var for the target, refuse without it), snapshot + restore the cvars it touches, and only delete a favorite it actually added | `frontend/e2e-test.js`, `frontend/package.json` (`testall`), `justfile` (`fe-test`) |
| 3 | Plan 12 T4 step 2 text still says `rcon set sv_maplist ""` | Invalid as written (see Notes) — use `set sv_maplist ,` | `12_sv_maplist_resilience.md` T4 |

## Live Verification Log

2026-10-03, server noir.lan (yquake2 8.70, game built 2026-07-11), API pid 549337 started
13:43:46 on cosmo by the operator, rotation queue q2dm1–q2dm8 (mode Random). All commands
via `POST /api/rcon/execute`; times PDT.

1. **API up / startup sync — ANOMALY.** 13:45:00 and 13:45:37: `sv_maplist` →
   `"q2dm1"` (expected all 8). 13:46:02 sent the API's exact push
   (`set sv_maplist q2dm1,…,q2dm8`) → read back all 8; held through 13:48:38 across three
   watchdog ticks (no spurious re-push). qbots fleet (`competition --roster
   ./roster-lite.yaml`, 4 bots, beacon connected) started 13:44:47.
2. **Drift detection — PASS.**
   - 13:49:26 `set sv_maplist ""` → `usage: set <variable> <value> [u / s]`; cvar unchanged.
   - 13:49:28 `set sv_maplist ,` → read `","` → **restored to all 8 at 13:49:50** (22 s).
   - 13:49:51 `set sv_maplist q2dm1` → read `"q2dm1"` → **restored at 13:50:51** (60 s).
3. **Guards — PASS** (13:51:32). `{"command":"map"}`, `"map   "`, `"gamemap \"\""` →
   HTTP 400 each, with `refusing 'map'/'gamemap' with an empty map name …` ERROR lines on
   the log stream; `POST /api/rotation {"map_name":"bad name"}` → 400, queue unchanged,
   `rotation.yaml` clean. Server stayed on q2dm1 with 4 players. Control
   `map q2dm2` → 200, server reloaded; at 13:52:01 on q2dm2 with all 4 bots, `sv_maplist`
   intact, clock anchor `exact`.
4. **Fraglimit match end with the fleet — PASS** (run 2).
   - Run 1, 13:52:25–14:02:25: fraglimit 5; top score never passed 3; restored to 0.
   - Run 2: 14:02:58 fraglimit 3 (top score was 3) → **14:03:06 server on q2dm3**, the
     `sv_maplist` successor of q2dm2 — online, 4 players, scores reset. No `Rotating to`
     line on the log stream, so the rotator did not do it. fraglimit restored to 0 at
     14:03:06; at 14:03:37 still q2dm3, all 4 bots, `sv_maplist` all 8.
5. **UI spot-check — not run** (see Notes).
