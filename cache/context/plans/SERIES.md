# pkgcache Plan Series

This document tracks the dependency chain and status of all plans for the pkgcache
project. **Read this before creating a new plan** — it holds the next free plan number
and the backlog rationale.

**Next free plan number: `02`.**

## Plan Dependencies

```
01_initial_nginx_proxy (Foundation — shipped pre-plan-system)
    ├── 02_cache_observability          (independent)
    ├── 03_additional_distros           (independent; sub-plan per distro)
    │     ├── 03_1_opensuse_zypper
    │     ├── 03_2_arch_pacman
    │     └── 03_3_ubuntu
    ├── 04_regional_fedora_mirror       (independent)
    ├── 05_prefetch_warming             (wants 02 for hit-ratio measurement)
    └── 06_cache_tls                    (only if the LAN stops being trusted)
```

## Plan Status

| # | Plan | Status | Depends On |
|---|------|--------|------------|
| 01 | Initial nginx caching proxy (Debian 13 + Fedora 44) | `done` (pre-plan) | N/A |
| 02 | Cache observability (`/stats`, HIT ratio, on-disk size) | `pending` | 01 |
| 03 | Additional distros (openSUSE / Arch / Ubuntu / EPEL / rpmfusion) | `pending` | 01 |
| 04 | Regional Fedora mirror instead of the master | `pending` | 01 |
| 05 | Prefetch / cache warming on a timer | `pending` | 01, 02 |
| 06 | HTTPS on the cache itself | `pending` | 01 |

> **Plan 01 shipped before this plan system existed** (commit `d814dc82b`, 2026-07-18) —
> there is no `01_*.md` to read. Its design rationale lives in `README.md` (the
> alternatives table), `CLAUDE.md` (Critical Facts), and `context/high_level.md`. Do not
> retroactively fabricate a Plan 01 file; treat those three as its record.

## Backlog Rationale

Why each pending plan exists, so whoever picks one up starts with context:

- **02 — Observability.** Right now the only way to know the cache works is a manual
  `curl -sI … | grep X-Cache-Status`. There is no answer to "what's my hit ratio?" or
  "how big is the cache?" without shelling into the host. Cheapest useful version: parse
  the existing `cache=` field in the `log_format cache` access log. Prerequisite for
  measuring whether Plan 05 is worth anything.

- **03 — Additional distros.** Each is the same shape: a route block with the
  metadata/package TTL split, plus a reversible `scripts/fix-<distro>`. Sub-plan per
  distro so they can land independently. Note **Arch** differs meaningfully — `pacman`
  mirrors are configured as `Server = …` lines in `/etc/pacman.d/mirrorlist`, and package
  files are `*.pkg.tar.zst`; **EPEL/rpmfusion** are metalink-based like Fedora and slot in
  as extra `baseurl` repos.

- **04 — Regional Fedora mirror.** `/fedora/` currently points at
  `dl.fedoraproject.org`, Fedora's **master** mirror. Fine for a handful of machines,
  impolite at any real volume. Blocked on picking a mirror with a stable path shape;
  changing it means updating **both** the prefix `proxy_pass` and the `.rpm`
  sub-location's `rewrite` together (see `context/pitfalls.md`).

- **05 — Prefetch / warming.** Today the first client of the day pays full price for every
  metadata refresh and any new package. A timer that pulls metadata (and optionally hot
  packages) would hide that. Do **not** start this before 02 — without a hit-ratio number
  there is no way to tell if it helped.

- **06 — Cache TLS.** Low value: GPG already provides end-to-end integrity, which is the
  actual threat model on a home LAN. Worth doing only if the cache is ever exposed beyond
  it. Recorded so the "shouldn't this be HTTPS?" question has a written answer.

## Execution Order

No hard ordering beyond the dependencies above — these are independent improvements to a
shipped, working system. Suggested by value:

1. **02** (observability — makes everything else measurable)
2. **03** (whichever distro is actually on the LAN)
3. **04** (be a good mirror citizen)
4. **05**, then **06** only if the threat model changes

## Completed Plans

Completed plans are moved to `context/plans/completed/` (RULES.md Rule C).
