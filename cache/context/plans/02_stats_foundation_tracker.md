# Stats Foundation — Tracker

## Overview
- Status: 17% complete (1 of 6 tasks)
- Start date: 2026-07-18
- Test endpoint: `http://localhost:8080` with `CACHE_DIR=/tmp/pkgcache-test`
- Live deployment: `noir.lan:3129` (host cache dir `/main/docker/cache/data`) — **not**
  touched by this plan; T3's `run` changes only take effect on the next deploy.

## Resume Instructions

Read Plan 02 in full first — the Context section carries the two decisions everything
downstream depends on (`$request_uri` not `$uri`; `$msec` as the only time source), and
getting either wrong silently corrupts every number in Plans 03–05.

T1 is committed (`7d14ecd1f`). Start at **T2**.

The tasks are ordered so the repo is never half-moved: T2 moves files, T3 makes the
scripts work with the new layout, T4 is the actual feature, T5/T6 are documentation. Do
not reorder T2 and T3 — `./build` is broken between them, which is why they are adjacent
and small.

**The single most important check in this plan** is T4's:
```bash
awk -F'\t' '$9 ~ /\.rpm$/ {print $9}' "$L"/access-*.log | head
```
If those paths start `/pub/fedora/` instead of `/fedora/`, the log is using `$uri` and
every Fedora package will be mis-bucketed. Stop and fix before touching Plan 03.

While you are on the live host for anything, run `findmnt -no FSTYPE /main/docker/cache/data`
and record it below — Plan 03's SQLite WAL mode depends on it being a local filesystem.

## Progress

| # | Task | File | Status | Notes |
|---|------|------|--------|-------|
| 1 | T1: renumber SERIES for stats subsystem | `context/plans/SERIES.md` | done | 02–05 stats, backlog → 06–09, prefetch repointed at 05; commit `7d14ecd1f` |
| 2 | T2: move proxy files into `proxy/` | `Dockerfile`, `nginx.conf`, `conf.d/`, `.dockerignore` | pending | `git mv` only; do not rename the published image |
| 3 | T3: `all\|proxy\|stats` target arg; provision `stats/{logs,db}` | `build`, `run`, `publish` | pending | `mkdir`+`chown` is mandatory — nginx won't create the log dir |
| 4 | T4: `log_format stats` + dated access log | `proxy/nginx.conf` | pending | the feature; all verification weight is here |
| 5 | T5: doc debt — 26 refs + 2 Rule D obligations | 6 `.md` files | pending | most likely task to be half-done |
| 6 | T6: harvest to `distilled.md` / `pitfalls.md` | `context/*.md` | pending | + answer distilled Open Question 3 |

## Environment Facts (fill in during execution)

| Fact | Value | When |
|---|---|---|
| `findmnt -no FSTYPE /main/docker/cache/data` | *(unrecorded)* | — |
| WAL viable for Plan 03? | *(unrecorded — depends on above)* | — |
| `$uri` vs `$request_uri` confirmed live | *(unrecorded)* | — |
| `escape=default` keeps 9 fields with a tab in the URI | *(unrecorded)* | — |
| `X-Cache-Status` present on regex sub-locations (distilled OQ3) | *(unrecorded)* | — |
| Rootless podman working on the dev host? | **no** — `podman system migrate` needed (subuid); docker used instead | 2026-07-18 |

## Notes / Deviations

- **T1 was completed during the planning session**, before the plan file itself existed,
  because the renumbering had to land before any `git mv` and it touches only `SERIES.md`.
  Recorded here rather than silently backdated into the plan.
- **Rootless podman is currently broken on the dev host** — image pulls fail with
  `potentially insufficient UIDs or GIDs available in user namespace … requested 0:42`,
  suggesting `podman system migrate`. All 2026-07-18 verification was done with `docker`.
  This matters for T3/T4 verification commands (`docker logs` vs `podman logs`) and it
  means the `--userns=keep-id` path in `run` is **unverified on this host**.
