# Stats Foundation — Tracker

## Overview
- Status: **100% complete** (6 of 6 tasks)
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
| 2 | T2: move proxy files into `proxy/` | → `proxy/` | **done** | `git mv` only; build verified from the new context; `4c71e7fa6` |
| 3 | T3: target arg; provision the three dirs | `build`, `run`, `publish` | **done** | avoided a `set -e` trap in the guard; `cc3097fe9` |
| 4 | T4 + T4a: stats log format, dated log, `root` | `proxy/nginx.conf`, `proxy/conf.d/pkgcache.conf` | **done** | 9 fields; `.rpm` URI = `/fedora/`; `94380d511` |
| 5 | T5: doc debt + 2 Rule D obligations | 9 `.md` files | **done** | 42 refs, not the estimated 26; `a381555f9` |
| 6 | T6: harvest + resolve open questions | `context/*.md`, `proxy/conf.d/` | **done** | found+fixed a live 30d TTL defect; `2c937c524` |

## Environment Facts (fill in during execution)

| Fact | Value | When |
|---|---|---|
| `findmnt -no FSTYPE /main/docker/cache/data` | *(unrecorded — check on the live host)* | — |
| WAL viable for Plan 03? | *(unrecorded — depends on above)* | — |
| `X-Cache-Status` present on regex sub-locations (distilled OQ3) | **yes** — real `.deb` and `.rpm` both return HIT | 2026-07-18 |
| **`$uri` vs `$request_uri`** | **RESOLVED** — diverge only on `.rpm`; `$uri` gives `/pub/fedora/…`. Use `$request_uri`. | 2026-07-18 |
| **`escape=default` keeps 9 fields** | **RESOLVED** — held across HIT/MISS/404/HEAD/banner/`%09%22` | 2026-07-18 |
| **Variable-path `access_log` needs a valid `root`** | **RESOLVED** — logs nothing without it; added T4a | 2026-07-18 |
| **`$upstream_bytes_received` > `$body_bytes_sent` on MISS** | **RESOLVED** — 5966 vs 6499; changed the `bytes_saved` formula | 2026-07-18 |
| Rootless podman working on the dev host? | **no** — `podman system migrate` needed (subuid) | 2026-07-18 |
| Container engine for verification | **docker** here; **podman** on `noir.lan` (no docker there) | 2026-07-18 |
| `--userns=keep-id` path verified? | **no — cannot be, on this machine.** Rootless-podman-only; unverified until the first live deploy. | 2026-07-18 |

## Notes / Deviations

- **T1 was completed during the planning session**, before the plan file itself existed,
  because the renumbering had to land before any `git mv` and it touches only `SERIES.md`.
  Recorded here rather than silently backdated into the plan.
- **Rootless podman is currently broken on the dev host** — image pulls fail with
  `potentially insufficient UIDs or GIDs available in user namespace … requested 0:42`,
  suggesting `podman system migrate`. All 2026-07-18 verification was done with `docker`.
  The live host `noir.lan` is the opposite: **podman, no docker**. So verify with
  `RUNTIME=docker` here, and **do not change the scripts' podman-preferred detection** —
  that is what makes them work unchanged in production. Consequence: the
  `--userns=keep-id` path in `run` is **unverifiable here** and stays unverified until the
  first live deploy.

- **Four Key Facts were resolved live on 2026-07-18, before implementation began**, by
  probing a config that reproduced this repo's Fedora/Debian location blocks. Two of them
  changed the plan:
  1. **`$uri` vs `$request_uri`** — confirmed exactly as feared. Divergence is `.rpm`-only,
     so 3 of 4 test cases agree and a casual check would have passed. The T4 assertion is
     now a *regression guard* rather than an experiment.
  2. **A variable-path `access_log` writes nothing without a valid `root`** — new, and a
     genuine blocker. nginx starts, `nginx -t` passes, caching works, and the log file is
     never created; the only signal is `testing "/etc/nginx/html" existence failed` in the
     error log. The image has no `/etc/nginx/html` and a pure reverse-proxy server block
     has no reason to declare a `root`. **Added T4a.** Without this the whole stats
     subsystem would have produced an empty dashboard with everything appearing healthy.
  3. **`$upstream_bytes_received` exceeds `$body_bytes_sent` on a MISS** (headers are
     counted) — so the planned whole-window `Σ served − Σ upstream` for "bytes saved" was
     wrong. Corrected to a hit-class-only subtraction; propagated to Plans 03 and 04.
  4. **HEAD requests log 0 bytes with a real cache status** — confirms the ratio-distortion
     risk was real, not theoretical.

- **Operator-supplied production log** (2026-07-18) confirms real traffic from
  `192.168.10.99` is overwhelmingly `.rpm`, with a healthy HIT/MISS mix and percent-encoded
  filenames (`usbmuxd-1.1.1%5e2025…`). That is precisely the traffic the `$uri` bug would
  have mis-filed.

## Outcome

All six tasks done and verified. Two things the plan did not anticipate:

1. **T5 was bigger than estimated** — 42 stale path references, not 26. The estimate
   counted only `.md` prose; it missed occurrences inside code blocks and Sources lists.
   The plan's own advice ("re-run the grep rather than trusting this table") is what
   caught it.

2. **T6 found a live defect, not just documentation.** Answering the open question about
   upstream `Cache-Control` revealed that **every Debian package had been cached 30 days
   instead of the configured 365** since the proxy shipped — Fastly's
   `max-age=2592000` outranks `proxy_cache_valid`. Fedora sends no `Cache-Control`, so it
   was correct, which is why the asymmetry stayed invisible. Fixed and re-measured within
   the task; see `distilled.md`.

**Not verified here and still open:** the `--userns=keep-id` path, which only exists under
rootless podman. It is unverifiable on this dev machine and stays that way until the first
live deploy on `noir`.
