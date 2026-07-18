# pkgcache Plan Series

This document tracks the dependency chain and status of all plans for the pkgcache
project. **Read this before creating a new plan** — it holds the next free plan number
and the backlog rationale.

**Next free plan number: `06`.**

## Plan Dependencies

```
01_initial_nginx_proxy (Foundation — shipped pre-plan-system)
    │
    └── 02_stats_foundation        (repo split + nginx stats log format; NO Rust)
          └── 03_stats_ingest      (Rust workspace + log reader + SQLite; NO HTTP)
                └── 04_stats_api   (axum + snapshot + container)
                      └── 05_stats_dashboard   (React frontend)
                            └── 08_prefetch_warming

    ├── 06_additional_distros      (independent; sub-plan per distro)
    │     ├── 06_1_opensuse_zypper
    │     ├── 06_2_arch_pacman
    │     └── 06_3_ubuntu
    ├── 07_regional_fedora_mirror  (independent)
    └── 09_cache_tls               (only if the LAN stops being trusted)
```

## Plan Status

| # | Plan | Status | Depends On |
|---|------|--------|------------|
| 01 | Initial nginx caching proxy (Debian 13 + Fedora 44) | `done` (pre-plan) | N/A |
| 02 | Stats foundation: repo split + nginx stats log format | `pending` | 01 |
| 03 | Stats: Rust ingest core (log reader → SQLite) | `pending` | 02 |
| 04 | Stats: API + snapshot + container | `pending` | 03 |
| 05 | Stats: dashboard frontend | `pending` | 04 |
| 06 | Additional distros (openSUSE / Arch / Ubuntu / EPEL / rpmfusion) | `pending` | 01 |
| 07 | Regional Fedora mirror instead of the master | `pending` | 01 |
| 08 | Prefetch / cache warming on a timer | `pending` | 01, 05 |
| 09 | HTTPS on the cache itself | `pending` | 01 |

> **Plan 01 shipped before this plan system existed** (commit `d814dc82b`, 2026-07-18) —
> there is no `01_*.md` to read. Its design rationale lives in `README.md` (the
> alternatives table), `CLAUDE.md` (Critical Facts), and `context/high_level.md`. Do not
> retroactively fabricate a Plan 01 file; treat those three as its record.

> **Renumbering note (2026-07-18).** Plans 02–06 were previously a flat backlog
> (observability / distros / mirror / prefetch / TLS). The observability item grew into a
> four-plan stats subsystem (02–05), so the remaining backlog shifted to 06–09. No plan
> files existed at the time, so nothing was renamed and no cross-references broke.

## The Stats Subsystem (Plans 02–05)

A second container reads nginx's access logs off the shared volume, aggregates them into
SQLite, and serves a dashboard on **:8081** — per-client hit/miss ratios, bytes saved,
time series, and top packages. It is coupled to the proxy **only through the filesystem**:
no shared network, no service discovery. If the stats container is down for a week nothing
is lost — it catches up.

The split into four plans is deliberate: each ends at a point that can be **verified
observably on its own**, per `RULES.md` Rule A. In particular the 03/04 boundary exists so
that `pkgcache-stats --once` can be cross-checked against `awk` over the same log file
before any HTTP code exists — that byte-for-byte comparison is the strongest verification
available anywhere in this project.

| # | Ends when | Rule A gate |
|---|---|---|
| 02 | Real 9-field TSV lines are on disk | field count is exactly 9; a `.rpm` line's URI field starts `/fedora/` |
| 03 | `stats.sqlite` holds correct numbers | `sqlite3` sums == `awk` sums, to the byte |
| 04 | `:8081/api/stats` serves those numbers | `jq .kpis` matches sqlite; ETag re-request → 304 |
| 05 | The dashboard renders them | live in a browser at phone width, updating every 5s |

**Parallelization seam:** Plan 04 freezes the JSON payload schema and commits
`stats/frontend/src/lib/fixture.json`. Plan 05 can then be built entirely against the
fixture with `npm run dev` and no backend running. That is the only real concurrency
available in the chain.

## Backlog Rationale

Why each remaining pending plan exists, so whoever picks one up starts with context:

- **06 — Additional distros.** Each is the same shape: a route block with the
  metadata/package TTL split, plus a reversible `scripts/fix-<distro>`. Sub-plan per
  distro so they can land independently. Note **Arch** differs meaningfully — `pacman`
  mirrors are configured as `Server = …` lines in `/etc/pacman.d/mirrorlist`, and package
  files are `*.pkg.tar.zst`; **EPEL/rpmfusion** are metalink-based like Fedora and slot in
  as extra `baseurl` repos.

- **07 — Regional Fedora mirror.** `/fedora/` currently points at
  `dl.fedoraproject.org`, Fedora's **master** mirror. Fine for a handful of machines,
  impolite at any real volume. Blocked on picking a mirror with a stable path shape;
  changing it means updating **both** the prefix `proxy_pass` and the `.rpm`
  sub-location's `rewrite` together (see `context/pitfalls.md`).

- **08 — Prefetch / warming.** Today the first client of the day pays full price for every
  metadata refresh and any new package. A timer that pulls metadata (and optionally hot
  packages) would hide that. **Depends on Plan 05, not merely 02** — without the dashboard
  there is no hit-ratio number to tell whether the prefetching helped, and this is exactly
  the kind of change that feels effective while doing nothing.

- **09 — Cache TLS.** Low value: GPG already provides end-to-end integrity, which is the
  actual threat model on a home LAN. Worth doing only if the cache is ever exposed beyond
  it. Recorded so the "shouldn't this be HTTPS?" question has a written answer.

## Execution Order

1. **02 → 03 → 04 → 05** — the stats subsystem, strictly in order (05 may start against
   the fixture once 04 freezes the schema).
2. **06** (whichever distro is actually on the LAN) and **07** (be a good mirror citizen)
   — independent, any time.
3. **08** once the dashboard can measure it; **09** only if the threat model changes.

## Completed Plans

Completed plans are moved to `context/plans/completed/` (RULES.md Rule C).
