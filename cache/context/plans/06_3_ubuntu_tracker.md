# Ubuntu 22.04 (jammy) route + client container — Tracker

## Overview
- Status: 67% complete (T1, T2 done)
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
| 1 | T1: `/ubuntu/` + `/ubuntu-security/` routes | `proxy/conf.d/pkgcache.conf` | done | clamp measured 3300s -> 60s; sec `.deb` 200 not 404 |
| 2 | T2: `containers/ubuntu` image + build/publish | `containers/ubuntu/*` | done | 22.04 classic + 24.04 deb822 both verified |
| 3 | T3: docs + Rule D harvest | `README.md`, `CLAUDE.md`, `context/*.md` | pending | SERIES.md marks 06_3 done |

## Notes / Deviations

- **T1 confirmed the Pre-Identified Bug by measurement, not argument.** A throwaway
  image with the metadata clamp removed gave `valid_sec - date = 3300`; with it, `60`.
  The +68 s request returned `REVALIDATED` (it would have said `HIT` for 55 min without
  the clamp). Both proofs are in the T1 commit message.
- **Incidental finding, recorded in README:** Debian metadata is cached **120 s**, not the
  60 s the config appears to say — `deb.debian.org` sends `max-age=120` and upstream
  Cache-Control outranks `proxy_cache_valid`. Harmless (still short, still revalidated) but
  the README said 60 s, which was not true. Not "fixed" in the config: 120 s is upstream's
  own freshness signal and shortening it would only add requests.
- Local test proxy runs on **:8090**, not :8080 — an unrelated `opencode` container owns
  8080 on this dev box.
- **T2 deviation (good news): the `UBUNTU_RELEASE` knob is verified on 24.04 too,** not
  just 22.04 as the plan scoped. The same script handles deb822 `ubuntu.sources`.
- **T2 bug, caught by the script's own assertion.** The post-rewrite check was a recursive
  `grep` over `/etc/apt/sources.list.d/`, which on 24.04 matches the stock URLs in our own
  `.pkgcache-bak` backup (it sits in that directory there; on 22.04 it sits beside
  `sources.list`, outside it) — so a correct rewrite reported failure and the build
  aborted. Fixed by verifying over the same file list used for the rewrite. Worth noting
  the assertion did its job: the failure was loud, at build time, instead of shipping a
  half-configured image. Harvested to pitfalls.md in T3.
