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

- **T1**: implemented as `spawn_sv_maplist_watchdog(state)` (a named fn) rather than
  an inline block in `main()`, so it reads like the existing `spawn_sv_maplist_sync`.
  Design constraints from the plan all hold: 60 s interval, never push an empty
  queue, never push on an unparseable reply, not gated on `rotation_enabled`.
- **T2 / Open Question 1**: `frontend/src/lib/api.ts:executeRcon` throws a generic
  error on any `!res.ok`, so a bare `StatusCode::BAD_REQUEST` (no JSON body) keeps
  the UI's error path working. The rejection is also broadcast on the log stream as
  an `ERROR` line, so the in-app console says why nothing happened.
- **T3 test runner is broken in this environment, and it was broken before Plan 12**:
  `npm run test` and `npm run build` both **segfault** — `require('vite')` crashes on
  load (vite 8 vs. the vitest 2.1.9 / `node_modules.gentoo` install; `npm ls` reports
  invalid peers). `npm run lint` fails for a separate pre-existing reason: `eslint .`
  descends into the `node_modules` → `node_modules.gentoo` symlink and dies on a
  missing `@tanstack/eslint-config`.
  Verified around it: `tsc -b` exits 0, `npx eslint` is clean on all four touched
  files, and the 7 `buildApplyCommands` cases were compiled with `tsc` and run under
  `node --test` — **7/7 pass**. The vitest suite is committed as specified and should
  go green once the frontend toolchain is repaired (worth its own small plan: pin
  vitest to a Vite-8-compatible major and make eslint ignore `node_modules.gentoo`).
- Pre-existing `dead_code` warning on `LogStream::get_history` (`crates/api/src/logs.rs:83`)
  is untouched — not introduced here, and out of this plan's scope.

## Live Verification Log

(record T4 protocol results here, step by step, with timestamps)
