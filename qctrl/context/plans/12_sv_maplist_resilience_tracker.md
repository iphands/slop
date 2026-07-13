# sv_maplist Resilience + Empty-Map Guards — Tracker

## Overview
- Status: 75% complete (T1–T3 committed; T4 needs the live server + operator)
- Start date: 2026-07-12
- Incident reference: server died on `ERROR: Couldn't load maps/.bsp` (2026-07-12)
  when a qbots fleet ended a fraglimit match while `sv_maplist` was empty.
  Full forensics: `../qbots/context/plans/64_map_change_survival_tracker.md`.
- Live target: Q2 server on cosmo (rcon via qctrl API at cosmo.lan:3000).

## Resume Instructions
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
| 4 | T4: live regression protocol | — | pending | needs the live server + a qbots fleet; operator restarts the API |

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

## Live Verification Log

(record T4 protocol results here, step by step, with timestamps)
