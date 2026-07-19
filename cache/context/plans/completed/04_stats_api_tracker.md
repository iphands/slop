# Stats API + Container — Tracker

## Overview
- Status: **100% complete** (5 of 5 tasks)
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
| 1 | T1: snapshot builder | `crates/stats/src/snapshot.rs` | **done** | `6946bc69b` |
| 2 | T2: freeze schema + fixture | `frontend/src/lib/fixture.json` | **done** | `4736da7f6`; a Rust test asserts it still deserializes |
| 3 | T3: axum router | `crates/stats/src/api.rs` | **done** | `6946bc69b`; 304 + gzip + /api/typo 404 all verified live |
| 4 | T4: container | `stats/Dockerfile` | **done** | `4736da7f6`; **14.8 MB** built |
| 5 | T5: wire into `./run` | `run` | **done** | `./run all` brings up both, both healthy |

## Measurements to Record

| Metric | Value | When |
|---|---|---|
| Payload size (raw / gzipped) | 43,590 B raw | T2 |
| Snapshot rebuild duration on real data | *(unrecorded)* | T1 |
| Image size | **14.8 MB** | T4 |
| Cold / warm container build time | *(unrecorded)* | T4 |

## Notes / Deviations

*(none yet)*

## Outcome

All five tasks done and verified live end to end:

```
/healthz            {"status":"ok","lag_seconds":2}
/api/stats          17,470,163 bytes saved; package 12 reqs @0.83, metadata 12 @0.82
                    top packages parsed to names: glib2, cowsay
                    logs_readable true, parse_errors 0, cache_disk reported
ETag re-request     304
Accept-Encoding     real gzip
/api/typo           404 (not the SPA shell)
index.html          no-cache; /assets/*.js immutable
container           14.8 MB, healthy, USER 1000:1000
isolation           no shared network; /cache mounted rw=false
```
