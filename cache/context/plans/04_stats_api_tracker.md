# Stats API + Container — Tracker

## Overview
- Status: 0% complete (0 of 5 tasks)
- Start date: *(not started)*
- Depends on: Plan 03 complete — **its awk-vs-sqlite cross-check must have passed exactly**,
  not approximately. Serving numbers you haven't proven correct just moves the bug.
- Test endpoint: `http://localhost:8081`
- Test data: `/tmp/pkgcache-test/stats/` populated by Plan 03

## Resume Instructions

Read Plan 04's Context first. The performance property everyone will ask about
("is it fast?") comes entirely from the snapshot: SQLite is never touched by the polled
endpoint, the JSON and its gzip are built once per tick, and a request is a refcount bump.
If a change puts a query in `get_stats`, the design is gone.

**T2 is the seam.** Freezing the payload schema and committing `fixture.json` unblocks
Plan 05 to run in parallel against `npm run dev` with no backend. Do T1 and T2 before
anything else if someone is waiting to start the frontend.

Two things that are easy to get subtly wrong and hard to notice later:
- ratios must serialize `null` (not `0.0`) on a zero denominator, or a cold cache looks broken;
- HEAD requests must be excluded from byte ratios, or your own `curl -sI` testing distorts
  the metric while you're testing it.

T5's negative test — deliberately making the logs dir unreadable and confirming
`logs_readable: false` plus a loud ERROR — is worth more than any other single check here.
That is the failure mode a uid mismatch produces in production, and untested it presents as
a dashboard of zeros with nothing in the logs.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: snapshot builder | `crates/stats/src/snapshot.rs` | pending | all 3 windows in one payload; rebuild every tick |
| 2 | T2: freeze schema + fixture | `frontend/src/lib/fixture.json` | pending | **unblocks Plan 05 in parallel** |
| 3 | T3: axum router | `crates/stats/src/{api,assets}.rs` | pending | ETag/304; rust-embed; `/api` 404 ≠ SPA |
| 4 | T4: container | `stats/Dockerfile` | pending | 3-stage, static musl, ~15 MB |
| 5 | T5: wire into `./run` | `run`, `README.md` | pending | same uid + same userns flags as the proxy |

## Measurements to Record

| Metric | Value | When |
|---|---|---|
| Payload size (raw / gzipped) | *(unrecorded)* | T2 |
| Snapshot rebuild duration on real data | *(unrecorded)* | T1 |
| Image size | *(unrecorded)* | T4 |
| Cold / warm container build time | *(unrecorded)* | T4 |

## Notes / Deviations

*(none yet)*
