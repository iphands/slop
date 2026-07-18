# Plan 02 — Stats Foundation: repo split + nginx stats log format

> **Status**: in-progress (T1 done)
> **Created**: 2026-07-18
> **Depends on**: Plan 01 (the shipped proxy)
> **Goal**: The proxy writes a machine-readable, dated TSV access log into a subtree the future stats container will own, and the repo is split into `proxy/` + `stats/` to hold two images.
> **Agent**: implementation agent

---

> **Before writing any config, re-read `context/plans/RULES.md` in full** — especially
> Rule A (prove it runs and caches; there is no compiler here).
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Restructure the repo for two container images and make nginx emit a dated,
9-field TSV stats log that a reader process can consume — **without writing any Rust**.

**Deliverables**:
1. `SERIES.md` renumbered for the stats subsystem *(done — commit `7d14ecd1f`)*
2. `proxy/` subdir holding the existing `Dockerfile`, `nginx.conf`, `conf.d/`, `.dockerignore`
3. `build` / `run` / `publish` accepting `all|proxy|stats`, and `run` creating + chowning
   `${CACHE_DIR}/stats/{logs,db}`
4. `log_format stats` + `map $time_iso8601 $logdate` + `open_log_file_cache` + a second
   `access_log` writing `stats/logs/access-YYYY-MM-DD.log`
5. All 26 broken doc references fixed, plus the two Rule D doc obligations this work
   falsifies (`CLAUDE.md` "not Rust", `high_level.md` "implementation language — none")
6. New findings recorded in `distilled.md` / `pitfalls.md`

**Estimated effort**: Small–Medium (half day)

---

## Context

`pkgcache` works but is unobservable — the only signal is a manual
`curl -sI … | grep X-Cache-Status` against one URL. Plans 02–05 build a stats container
that reads the proxy's access logs off the shared volume and serves a dashboard on :8081.
This plan is the part that touches the **proxy**; everything after it is new code in
`stats/` and never modifies the proxy again.

### Why a dated log file rather than syslog or a socket

The alternative was `access_log syslog:server=stats:514,…` pushing over UDP. Rejected:
it requires a shared network between the two containers, loses data whenever the stats
container is down, and makes the proxy's logging depend on another service being up. A
file on the shared volume means **zero coupling** — no network, no discovery, and a stats
container that has been down for a week simply catches up.

The cost of a file is rotation, which nginx cannot do itself. Rather than adding logrotate
(a signal, a sidecar, and a race with a concurrent reader) or `copytruncate` (a race by
construction), a **variable in the log path** makes rotation implicit: nginx opens a new
file when the date changes, and the reader prunes old ones.

### Why `$request_uri` and not `$uri` — this is the sharpest trap in the plan

`conf.d/pkgcache.conf`'s `.rpm` sub-location does `rewrite ^/fedora/(.*)$ /pub/fedora/$1 break;`
(it must — see `context/pitfalls.md`, first entry). Therefore **`$uri` for every Fedora
package is `/pub/fedora/…`**. A downstream classifier that takes "repo = first path segment"
would file every `.rpm` under a repo named `pub`, while metadata — served by the parent
location, which does no rewrite — classifies correctly.

The result would be a dashboard that looks about 90% right while silently mis-bucketing
**exactly the high-value traffic**. `$request_uri` is the original, pre-rewrite URI (and is
what the cache key uses). There is no situation in this project where `$uri` is the right
choice for logging.

### Why `$msec` is the only timestamp

`$time_iso8601` is local-time-with-an-offset. Parsing it into a database imports a whole
timezone bug surface for zero benefit. `$msec` is epoch seconds with milliseconds — no
zone, monotonic through DST, and correct for backfill when the reader catches up after
downtime. `$time_iso8601` is used **only** through the `map` that builds the filename.

Rule for `distilled.md`: **the ingest never infers time from a filename.**

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| `$uri` is post-`rewrite` | `.rpm` requests log `/pub/fedora/…` | nginx docs; **must be confirmed live in T4** |
| `$request_uri` is pre-`rewrite`, includes query string | `/fedora/…` | nginx docs |
| `$msec` | epoch seconds with ms, e.g. `1784500000.123` | nginx docs |
| `$upstream_bytes_received` | bytes read from upstream; `-` on a cache HIT | nginx docs |
| `$upstream_cache_status` unset on non-proxied locations | logs `-` (sometimes empty) | nginx docs |
| Variable in `access_log` path ⇒ open/close per request without `open_log_file_cache` | — | nginx docs |
| `access_log` declared at `server` level **replaces** the inherited `http` one | — | nginx docs |
| Regex `location` rejects `proxy_pass` with a URI part | `[emerg]` at parse time | **verified live 2026-07-18** (`distilled.md`) |
| Duplicate `:8080` `server` blocks are **not** an error | `nginx -t` passes; first wins silently | **verified live 2026-07-18** (`distilled.md`) |
| Doc references that break on the move | **26** across 6 files | `grep -rnoE` 2026-07-18 |
| `build`/`run`/`publish` contain **no** hardcoded config paths | they use `$(dirname $0)` + `.` build context | `grep` 2026-07-18 |

---

## Step-by-Step Tasks

### T1: Renumber `SERIES.md` for the stats subsystem

**Status: DONE** — commit `7d14ecd1f`.

**File**: `context/plans/SERIES.md`

**What to do**: Rescope 02 to this plan, add 03–05, shift the old backlog to 06–09, set
next-free to 06, repoint prefetch (now 08) at Plan 05.

**Why first**: it must land *before* any file moves, so no later commit has to touch both
the series doc and a `git mv`.

**Verify**: `SERIES.md` status table and dependency graph agree; no plan number appears twice.

---

### T2: Move the proxy into `proxy/`

**Files**: `Dockerfile`, `nginx.conf`, `conf.d/pkgcache.conf`, `.dockerignore` → `proxy/`

**What to do**:
```bash
mkdir -p proxy
git mv Dockerfile nginx.conf .dockerignore proxy/
git mv conf.d proxy/conf.d
```
Nothing inside those files needs editing — the `Dockerfile`'s `COPY nginx.conf …` and
`COPY conf.d/pkgcache.conf …` are relative to the build context, which becomes `proxy/`
in T3. Do **not** rename the image: `iphands/pkgcache` is already published and referenced
by the live deployment.

Update `proxy/.dockerignore` — it currently reads `*` + `!Dockerfile` + `!nginx.conf` +
`!conf.d/` + `!conf.d/**`, which still works unchanged once the context is `proxy/`.
Confirm rather than assume.

**Verify**:
```bash
./build proxy   # (after T3) or: docker build -t pkgcache:t proxy/
docker run --rm --entrypoint nginx pkgcache:t -t     # syntax is ok / test is successful
git status --short                                    # only renames, no deletions
```

**Commit**: `task(T2): move proxy files into proxy/`

---

### T3: `build`/`run`/`publish` take `all|proxy|stats`; `run` provisions the stats dirs

**Files**: `build`, `run`, `publish`

**What to do**:

Add a target argument to all three, defaulting to `all`. Keep the existing
podman-preferred engine detection and the `${VAR:-default}` knob discipline exactly as-is.

```bash
TARGET="${1:-all}"
case "$TARGET" in
    all|proxy|stats) ;;
    *) echo "usage: $0 [all|proxy|stats]" >&2; exit 2 ;;
esac
```

New knobs: `STATS_IMAGE="${STATS_IMAGE:-iphands/pkgcache-stats}"`,
`STATS_PORT="${STATS_PORT:-8081}"`, `STATS_NAME="${STATS_NAME:-pkgcache-stats}"`.

`build` builds `proxy/` and/or `stats/` as separate contexts. **`./build stats` must fail
cleanly with a clear message until Plan 04 adds `stats/Dockerfile`** — do not stub a
Dockerfile that produces a broken image.

`run` gains, **before launching either container**:
```bash
mkdir -p "$CACHE_DIR"/stats/logs "$CACHE_DIR"/stats/db
chown -R "${APP_UID}:${APP_GID}" "$CACHE_DIR"/stats
```
This is not optional. nginx will **not** create the log directory itself when the path
contains a variable, and a missing directory produces one `error_log` line *per request*
plus zero stats — a stderr flood with a non-obvious cause.

The stats container's mounts (wired up fully in Plan 04, documented now):
- `-v "${CACHE_DIR}/stats:/data"` (rw) — it never sees the package cache
- `-v "${CACHE_DIR}:/cache:ro"` — **opt-in** via `STATS_CACHE_RO=1`, used only for a
  `statvfs` + `pkg/` subtree size so the dashboard can show "38 GB / 100 GB"

**Both containers must run as the same `APP_UID:APP_GID` and be launched with the same
userns flags.** Add a comment saying so next to `EXTRA_ARGS` — a mismatch means nginx
writes logs the stats container cannot read, and the failure mode is a dashboard of
silent zeros with no error anywhere.

**Verify**:
```bash
shellcheck build run publish                      # clean
./build              # builds proxy; reports stats not buildable yet
./build stats        # clean failure, actionable message
./build bogus        # usage message, exit 2
PORT=8080 CACHE_DIR=/tmp/pkgcache-test ./run
ls -ld /tmp/pkgcache-test/stats/logs /tmp/pkgcache-test/stats/db   # exist, right owner
```

**Commit**: `task(T3): build/run/publish take all|proxy|stats; run provisions stats dirs`

---

### T4: `log_format stats` + dated access log

**File**: `proxy/nginx.conf` (all of it at `http` level — **not** `conf.d/pkgcache.conf`)

**What to do**: add to the `http` block, after the existing `log_format cache`:

```nginx
    # Dated stats log for the pkgcache-stats service (Plans 02-05).
    #
    # $time_iso8601 is LOCAL time with an offset. Only its DATE is used, and only
    # to name the file. Every timestamp the reader stores comes from $msec (epoch),
    # so the ingest never has to know what timezone nginx thought it was in, and a
    # reader catching up after downtime still lands lines in the right buckets.
    map $time_iso8601 $logdate {
        "~^(?<ymd>\d{4}-\d{2}-\d{2})"  $ymd;
        default                         "nodate";
    }

    # Machine-readable TSV. Fields, in order:
    #   1 msec  2 remote_addr  3 method  4 status  5 body_bytes  6 upstream_bytes
    #   7 cache_status  8 request_time  9 request_uri
    #
    # $request_uri is the ORIGINAL, PRE-rewrite URI. $uri would be /pub/fedora/...
    # for every .rpm (conf.d/pkgcache.conf rewrites it -- see context/pitfalls.md)
    # and would file every Fedora package under a repo called "pub". Never $uri.
    #
    # $upstream_bytes_received is what we actually paid upstream; bytes saved is
    # derived as (body_bytes_sent - upstream_bytes_received), not guessed from the
    # cache status. It is "-" on a HIT.
    #
    # escape=default renders tabs/quotes/control chars in the URI as \xNN, so the
    # TSV framing survives a hostile request line. The one variable-length,
    # client-controlled field is LAST, so a malformed one cannot shift any other.
    log_format stats escape=default
        '$msec\t$remote_addr\t$request_method\t$status\t'
        '$body_bytes_sent\t$upstream_bytes_received\t$upstream_cache_status\t'
        '$request_time\t$request_uri';

    # MANDATORY with a variable in the access_log path: without it nginx open()s
    # and close()s the log file on EVERY request.
    open_log_file_cache max=32 inactive=1m valid=1m min_uses=1;

    access_log /dev/stdout cache;                                    # human, unchanged
    access_log /var/cache/nginx/stats/logs/access-$logdate.log stats;
```

**Both `access_log` lines stay at `http` level.** `access_log` is inherited only when the
current level declares none — putting either one inside `server` silently drops the other.

**Do not add `buffer=`.** nginx writes each line with a single `write(2)` to an `O_APPEND`
fd, which is atomic across workers; buffering would risk losing up to `buffer=` bytes on
SIGKILL and buys nothing at homelab request rates — `open_log_file_cache` already removed
the syscall cost that `buffer=` exists to solve.

`conf.d/pkgcache.conf` needs **no change**. Its `location = /healthz { access_log off; }`
already suppresses *both* logs, which is what we want — the container HEALTHCHECK is
~2,880 requests/day of pure noise.

**Verify** (this is the whole point of the plan; `nginx -t` is blind to all of it):
```bash
./build proxy && PORT=8080 CACHE_DIR=/tmp/pkgcache-test ./run && sleep 2
docker logs pkgcache | grep -iE 'emerg|error'          # expect nothing

curl -s  http://localhost:8080/debian/dists/trixie/InRelease >/dev/null
curl -s  http://localhost:8080/fedora/linux/releases/44/Everything/x86_64/os/repodata/repomd.xml >/dev/null
curl -s 'http://localhost:8080/debian/%09%22weird'          >/dev/null   # tab+quote in URI
# and one real .rpm and one real .deb

L=/tmp/pkgcache-test/stats/logs
ls "$L"                                            # access-YYYY-MM-DD.log exists
awk -F'\t' '{print NF}' "$L"/access-*.log | sort -u # MUST print exactly: 9
awk -F'\t' '$9 ~ /\.rpm$/ {print $9}' "$L"/access-*.log | head
#   ^ MUST start /fedora/  -- if it starts /pub/fedora/ the log uses $uri, STOP
grep -c healthz "$L"/access-*.log                  # 0 -- healthz is not logged
```

**Commit**: `task(T4): nginx stats log format + dated access log`

---

### T5: Repair the doc debt (26 references + 2 Rule D obligations)

**Files**: `README.md`, `CLAUDE.md`, `context/pitfalls.md`, `context/distilled.md`,
`context/plans/RULES.md`, `context/plans/NN_example.md`

**What to do**: measured 2026-07-18, **26** references to `Dockerfile` / `nginx.conf` /
`conf.d/pkgcache.conf` / `.dockerignore` break when those files move:

| File | Refs |
|---|---|
| `context/pitfalls.md` | 9 |
| `CLAUDE.md` | 5 |
| `context/plans/RULES.md` | 4 |
| `context/distilled.md` | 4 |
| `context/plans/NN_example.md` | 2 |
| `README.md` | 2 |

Re-run the grep rather than trusting this table:
```bash
grep -rnoE '(\./)?(conf\.d/pkgcache\.conf|nginx\.conf|Dockerfile|\.dockerignore)' \
    README.md CLAUDE.md context/ --include='*.md'
```

Also update the repo-layout tree in `CLAUDE.md` and the build/run examples in `README.md`
for the `all|proxy|stats` argument.

**Two Rule D obligations — statements this work makes false:**

1. **`CLAUDE.md`**, the sibling-projects note: *"This one is **not Rust** — it is config +
   shell… Don't reach for `cargo` here."* This becomes actively misleading the moment
   `stats/` exists. Rewrite to: the **proxy** is config-only; **`stats/` is Rust** — and
   say which verification gate applies to which half (Rule A's curl gate for the proxy,
   `cargo test`/clippy for `stats/`).
2. **`context/high_level.md` § "Implementation language — none (config + shell)"** — it
   argues *against* a Rust service ("a build, a test suite, a dependency tree, and a thing
   to keep patched") and names an explicit *revisit-if* trigger: **"if Plans 02
   (observability) and 05 (prefetch) both land and log-parsing shell becomes the ugly
   part."** That trigger is exactly what fired. Rewrite the section to record honestly
   that the trigger was pulled, what the shell alternative would have looked like
   (`awk` over a growing log with no offset tracking, no restart safety, no per-client
   aggregation), and why it lost. Add SQLite to the same file — it is the first database
   anywhere in the slop family.

**Verify**:
```bash
grep -rnE '(^|[^/])(conf\.d/|nginx\.conf|Dockerfile)' README.md CLAUDE.md context/ --include='*.md' \
  | grep -v 'proxy/' | grep -v 'stats/'      # every survivor is intentional
grep -rn 'not Rust' CLAUDE.md                # gone
grep -n 'Implementation language' context/high_level.md   # rewritten, not deleted
```

**Commit**: `task(T5): repoint docs at proxy/; retire the "not Rust" and "no build" claims`

---

### T6: Harvest findings (Rule D)

**Files**: `context/distilled.md`, `context/pitfalls.md`

**What to do**:

`distilled.md` — add, with `[LIVE 2026-07-18]` tags **only** for what T4 actually observed:
- `$uri` vs `$request_uri` and the `/pub/fedora/` mis-classification.
- **The ingest never infers time from a filename** — `$msec` is the only time source;
  `$time_iso8601` names the file. `TZ=UTC` in both containers.
- `bytes_saved = body_bytes_sent − upstream_bytes_received`, derived from two
  measurements rather than inferred from the cache status (a `REVALIDATED` response *did*
  cost an upstream round-trip, so the status-bucket heuristic is subtly wrong).
- `access_log` inheritance: declaring one at `server` level drops the `http` one.
- `open_log_file_cache` is mandatory with a variable in the path.
- Answer **Open Question 3** ("does `X-Cache-Status` appear on the regex sub-locations?")
  while you have a live proxy and real `.deb`/`.rpm` fetches — it costs one `curl -sI`.

`pitfalls.md` — new entry, and cross-reference it from the existing first entry (this is
the *same* nginx rewrite behavior biting from a new direction):
- **"Logging `$uri` mis-files every rewritten request"** — Problem / Fix / Sources.
- **"Deleting a log file nginx still holds an fd for"** — `open_log_file_cache valid=1m`
  means a worker keeps the fd for up to a minute after the last write; unlink it and nginx
  appends to an unreachable inode and the requests vanish silently. This is why Plan 03's
  retention floor is 3 days and never touches today's or yesterday's file.
- Extend the existing **rootless-podman** entry (do not write a second one) with: both
  containers must use the *same* userns flags, and the failure mode is silent zeros.

**Verify**: `grep -c '^# ' context/pitfalls.md` increased by 2; the new entries name real
files; nothing is tagged `[LIVE]` that was not actually observed.

**Commit**: `task(T6): record $request_uri, $msec and log-retention findings`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `proxy/nginx.conf` | `map`, `log_format stats`, `open_log_file_cache`, 2nd `access_log` | P0 |
| `run` | target arg; `mkdir`+`chown` of `stats/{logs,db}`; stats mounts documented | P0 |
| `build`, `publish` | `all\|proxy\|stats` target arg, `STATS_IMAGE` | P0 |
| `Dockerfile`, `nginx.conf`, `conf.d/`, `.dockerignore` | `git mv` → `proxy/` | P0 |
| `context/plans/SERIES.md` | renumber *(done, T1)* | P0 |
| `CLAUDE.md`, `context/high_level.md` | Rule D: retire "not Rust" / "no build" | P1 |
| `README.md`, `context/{pitfalls,distilled}.md`, `plans/{RULES,NN_example}.md` | repoint paths | P1 |
| `conf.d/pkgcache.conf` | **none** — verify it needs none | P2 |

---

## Open Questions / Risks

1. **`$uri` vs `$request_uri` is asserted from docs, not yet observed.** — *Mitigation:*
   T4's verification fetches a real `.rpm` and asserts field 9 starts `/fedora/`. If it
   starts `/pub/fedora/`, stop and fix before proceeding to Plan 03; every downstream
   number depends on it.
2. **`escape=default` behavior with tabs in a URI is asserted, not observed.** —
   *Mitigation:* T4 fetches `/debian/%09%22weird` and asserts the field count stays 9. A
   silently shifted field 9 would be very hard to diagnose later.
3. **The host filesystem may not support WAL.** SQLite WAL breaks on NFS/CIFS, and Plan 03
   depends on it. — *Mitigation:* run `findmnt -no FSTYPE /main/docker/cache/data` on the
   real host **during this plan**, while you're there, and record the answer in the
   tracker. If it isn't local, Plan 03 uses `journal_mode=TRUNCATE`.
4. **Log volume growth between now and Plan 03.** Nothing prunes the logs until the reader
   exists. — *Mitigation:* they're ~100 bytes/request; at homelab rates that's under a
   megabyte a day. Acceptable for the days between plans. Note it in the tracker so it
   isn't forgotten if Plan 03 stalls.
5. **`./build stats` has no Dockerfile until Plan 04.** — *Mitigation:* explicit, friendly
   failure, not a stub image.
6. **The doc-debt task is the most likely to be half-done.** 26 references across 6 files,
   and `RULES.md`'s own examples use `conf.d/pkgcache.conf` as the canonical `**File**:`
   value. — *Mitigation:* T5 ends with a grep whose every survivor must be intentional.

---

## Verification Checklist

- [ ] T1: `SERIES.md` renumbered; no duplicate plan numbers; next-free is 06
- [ ] T2: `git status` shows renames only; `docker build proxy/` succeeds
- [ ] T2: `nginx -t` in the rebuilt image prints `test is successful`
- [ ] T3: `shellcheck build run publish` clean
- [ ] T3: `./build bogus` exits 2 with a usage message; `./build stats` fails clearly
- [ ] T3: `./run` creates `stats/logs` + `stats/db` owned by `APP_UID:APP_GID`
- [ ] T4: container `Up`, no `emerg`/`error` in logs
- [ ] T4: **all six routes still MISS→HIT** (Rule A — the move must not have broken caching)
- [ ] T4: `awk -F'\t' '{print NF}' access-*.log | sort -u` prints exactly `9`
- [ ] T4: a `.rpm` line's field 9 starts `/fedora/`, **not** `/pub/fedora/`
- [ ] T4: a URI containing a tab still yields 9 fields
- [ ] T4: `/healthz` appears in neither log
- [ ] T4: `/dev/stdout` human log still works (`docker logs`)
- [ ] T5: no stale path references survive the grep unintentionally
- [ ] T5: `CLAUDE.md` no longer claims the project is not Rust
- [ ] T5: `high_level.md` records that its revisit-if trigger fired, and why
- [ ] T6: `distilled.md` + `pitfalls.md` updated; `[LIVE]` tags only on observed facts
- [ ] T6: `distilled.md` Open Question 3 answered
- [ ] All: host FS type recorded in the tracker (WAL viability for Plan 03)
- [ ] All: `README.md` updated in the same commit as the behavior change
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
