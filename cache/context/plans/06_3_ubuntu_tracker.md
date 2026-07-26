# Ubuntu 22.04 (jammy) route + client container — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-25
- Test endpoint: `http://localhost:8080` (local docker, `CACHE_DIR=/tmp/pkgcache-test`),
  with `containers/ubuntu` built against `CACHE=http://<host-ip>:8080` so the container
  can reach the test proxy. The live cache (`noir.lan:3129`) does **not** have the Ubuntu
  routes until this is deployed there.

## Resume Instructions

1. Read `06_3_ubuntu.md`, especially **Pre-Identified Bug** (`s-maxage=3300`) — the one
   non-obvious thing in this plan.
2. State check: `RUNTIME=docker ./build && RUNTIME=docker PORT=8080 CACHE_DIR=/tmp/pkgcache-test ./run proxy`,
   then `curl -sI localhost:8080/ | head -1` and `curl -s localhost:8080/` to see which
   routes the running config advertises.
3. Files in play: `proxy/conf.d/pkgcache.conf` (T1), `containers/ubuntu/*` (T2), docs (T3).
4. `containers/fedora/` is the reference implementation for T2 — copy its shape, not its
   dnf specifics.

## Progress

| # | Task | File | Status | Notes |
|---|------|------|--------|-------|
| 1 | T1: `/ubuntu/` + `/ubuntu-security/` routes | `proxy/conf.d/pkgcache.conf` | pending | metadata TTL clamp + security-pocket `rewrite` |
| 2 | T2: `containers/ubuntu` image + build/publish | `containers/ubuntu/*` | pending | classic `sources.list`, not deb822 |
| 3 | T3: docs + Rule D harvest | `README.md`, `CLAUDE.md`, `context/*.md` | pending | SERIES.md marks 06_3 done |

## Notes / Deviations

- Plan asserted nothing yet. Record here anything that turns out wrong — bluntly.
