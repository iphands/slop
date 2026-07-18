# Plan 04 — Stats: API + Snapshot + Container

> **Status**: pending
> **Created**: 2026-07-18
> **Depends on**: Plan 03 (correct numbers in `stats.sqlite`)
> **Goal**: `GET :8081/api/stats` serves a pre-serialized snapshot of those numbers in sub-millisecond time, from a self-contained container that runs alongside the proxy.
> **Agent**: implementation agent

---

> **Before writing any config, re-read `context/plans/RULES.md` in full** — especially
> Rule A (prove it runs and caches; there is no compiler here).
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Put an axum HTTP layer on the Plan 03 ingest service, serving a pre-built
snapshot, and ship it as a container wired into `./run`.

**Deliverables**:
1. Snapshot builder — five `GROUP BY` queries → payload → pre-serialized JSON **and**
   pre-gzipped bytes **and** an ETag, rebuilt every tick
2. axum router: `/api/stats`, `/api/stats/client/{ip}`, `/healthz`, SPA fallback
3. Multi-stage proxy/Dockerfile → static musl binary, ~15 MB image, non-root
4. `./run stats` launching it with the right mounts and uid
5. **A frozen payload schema + `stats/frontend/src/lib/fixture.json`** so Plan 05 can start
   in parallel

**Estimated effort**: Medium (1 day)

---

## Context

Plan 03 produces correct numbers. This plan makes them reachable. The performance
requirement ("SUPER fast") is met not by the language or the framework — at this data
volume anything would be instant — but by **never touching SQLite on the polled endpoint**.
The ingest tick already runs every 5 seconds; it rebuilds the entire dashboard payload at
the end of each tick, so a request is a refcount bump and a write to a socket.

### What the snapshot must *not* contain

A full per-client package list grows as `clients × distinct_packages` — which explodes
precisely on the day of a fleet-wide dist-upgrade, i.e. the day you most want the
dashboard. So the snapshot carries a **bounded top-10 per client**, and the full list moves
to `GET /api/stats/client/{ip}`, which *does* query SQLite. A human clicking a table row is
not a hot path, and that endpoint is not polled.

### Fixing qctrl's static-file wart rather than inheriting it

qctrl serves its frontend with `ServeDir::new("frontend/dist")` — a **relative** path — plus
a `middleware::from_fn` that string-matches `"/api/"` and re-reads `index.html` off disk on
every 404. The binary therefore only works from the right working directory, the SPA
fallback is implemented twice, and one of those paths returns HTML with no `Content-Type`.

Here: `rust-embed` compiles the assets into the binary (no disk, no CWD assumption), and
`.nest("/api", …)` with a nested `.fallback` lets axum's own routing separate API 404s from
SPA routes. `/api/typo` returns a real 404 instead of an HTML page.

### Why `/healthz` returns 503 on a stale tick

A stats service whose ingest task has panicked but whose HTTP server is still up is **worse
than one that is down**: the dashboard keeps rendering, showing frozen numbers, and nothing
alerts. `/healthz` returns 503 when `now − last_tick_at > 60s`. Note this works only
because Plan 03's tick advances its timestamp **even when zero lines were ingested** — an
idle cache stays healthy.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Payload size | ~25 KB raw, ~4 KB gzipped at 720 buckets + 10 clients | estimated; measure in T2 |
| Snapshot rebuild cost | 5 `GROUP BY` over ≤65k rows + serialize + gzip ≈ single-digit ms | estimated; measure in T1 |
| `Bytes::clone` | atomic refcount increment, not a memcpy | `bytes` crate docs |
| qctrl's SPA fallback | relative `frontend/dist` + `middleware::from_fn`, doubled | explored 2026-07-18 |
| Proxy image name | `iphands/pkgcache` — **already published, do not rename** | Plan 02 |
| Stats image name | `iphands/pkgcache-stats` | Plan 02 T3 |
| Base image convention | unprivileged, arbitrary uid, busybox `wget` HEALTHCHECK | `proxy/Dockerfile` |
| Container engine | **docker** to verify here (rootless podman is broken on this machine); **podman** on `noir.lan`, which has no docker. Use `RUNTIME=docker` locally; the scripts' auto-detection handles the live host. | 2026-07-18 |
| `--userns=keep-id` is **not verifiable** on the dev machine | it only applies to rootless podman | 2026-07-18 |

---

## Step-by-Step Tasks

### T1: Snapshot builder

**File**: `stats/crates/stats/src/snapshot.rs`

**What to do**: after each ingest tick, run the aggregate queries and build one payload
covering **all three windows** (24h / 7d / 30d). Shipping all three costs ~20 KB and
removes an entire class of "which window is this snapshot for" bugs — window switching in
the UI becomes pure client state with zero refetch.

Structure (see T2 for the JSON):
- `ingest` — `last_tick_at`, `lag_seconds`, `files_tracked`, `lines_ingested`,
  `parse_errors`, **`logs_readable`**, `db_bytes`, `data_free_bytes`
- `kpis` — `all` / `package` / `metadata` blocks for the 24h window, plus `lifetime` from
  the never-pruned `totals` table
- `series` — `24h` (24 hourly points), `7d` (168 hourly), `30d` (30 daily)
- `clients[]` — per-IP, split by kind, with a 24-point sparkline and **top-10** packages
- `top_packages[]` / `top_metadata[]` — top 25 by bytes
- `repos[]` — per-repo breakdown
- `cache_disk` — cache bytes, `max_size` cap, pct full, and free space on the host
  filesystem, from the `:ro` `/cache` mount (omitted only if `PKGCACHE_CACHE` is unset)

**`bytes_saved` is derived**, never stored — and it is **not** a whole-window subtraction:

```
bytes_saved = Σ over {HIT, REVALIDATED, STALE, UPDATING} of
                  max(0, body_bytes_sent − upstream_bytes_received)
```

Measured live 2026-07-18: `$upstream_bytes_received` **exceeds** `$body_bytes_sent` on a
MISS (it counts response headers), so `Σ served − Σ upstream` charges every MISS's header
overhead against the savings and can go negative on a quiet day. Subtracting within the hit
class only gets `HIT`, `REVALIDATED` and `MISS` all correct. See Plan 02 Key Facts and
Plan 03 T3 (`bytes_upstream_hit`). **Do not simplify this back.**

**Every ratio is `Option<f64>` and serializes as `null` when the denominator is zero.**
Rendering `0%` for "no requests yet" makes a healthy cold cache look broken — exactly the
failure the whole metadata/package split exists to prevent. The UI renders `—`.

**Exclude HEAD requests from byte ratios** (per Plan 03's stored `method`), or your own
`curl -sI` verification traffic quietly drags the denominator down — a distortion that only
appears while you are testing, which is the worst time for it.

Then:
```rust
pub struct Payload {
    pub json: Bytes,          // pre-serialized
    pub gzip: Bytes,          // pre-compressed, same tick
    pub etag: HeaderValue,
    pub generated_at: u64,
}
#[derive(Clone)]
pub struct Snapshot(Arc<RwLock<Arc<Payload>>>);
```
`RwLock<Arc<_>>`, not `arc-swap`: one write per 5s against a handful of reads is not
contention, and it matches qctrl's house style. A lock-free primitive here would be
performance theater.

**Rebuild every tick, unconditionally — even when zero lines were ingested.** Otherwise an
idle cache freezes the rolling 24h window at whatever it was when traffic stopped.

**Verify**: `cargo test -p pkgcache-stats snapshot::` — ratios are `None` on empty data;
`bytes_saved` never negative; a 30d series has exactly 30 points with gaps zero-filled.
Log the rebuild duration at `debug` and confirm it is single-digit ms on real data.

**Commit**: `task(T1): snapshot builder with pre-serialized payload`

---

### T2: Freeze the payload schema + commit the fixture

**Files**: `stats/crates/stats/src/snapshot.rs` (types),
`stats/frontend/src/lib/fixture.json`

**What to do**: this is the **parallelization seam** — the only real concurrency in the
02→05 chain. Once the JSON shape is frozen and a realistic sample is committed, Plan 05 can
be built entirely against the fixture with `npm run dev` and no backend running.

Generate the fixture from real data (`pkgcache-stats --dump-snapshot > fixture.json`), then
hand-edit it to include the awkward cases the UI must handle:
- a client with **zero** requests in the window (ratios `null` → renders `—`)
- a client seen only over IPv6
- `parse_errors > 0`
- `logs_readable: false`
- a 300 MB single-package MISS that tanks an otherwise healthy ratio
- an empty `top_packages` array
- **real-shaped package paths** — long, percent-decoded, with `+` in the version
  (`/debian/pool/main/libx/libxml2/libxml2-utils_2.12.7+dfsg+really2.9.14-2.1+deb13u3_amd64.deb`)
  and an rpm whose name contains dashes
  (`pipewire-jack-audio-connection-kit-libs-1.6.8-1.fc44.x86_64.rpm`). Synthetic short names
  like `foo.deb` would let Plan 05 ship a layout that breaks on the first real payload.
- **a realistic size spread** — production shows ~9,663x between the smallest and largest
  package in a single run (1.4 KB … 14 MB), with one package at 27% of total bytes

Document the shape in `stats/README.md`. Any later change to it is a breaking change for
Plan 05 and must update the fixture in the same commit.

**Verify**: `jq -e . fixture.json`; every field in the Rust type appears in the fixture;
`cargo test` asserts the fixture deserializes into the payload type (a real test, so the
two cannot drift silently).

**Commit**: `task(T2): freeze /api/stats payload schema + fixture.json`

---

### T3: axum router

**Files**: `stats/crates/stats/src/{api,assets}.rs`

**What to do**:

```rust
let api = Router::new()
    .route("/stats", get(get_stats))
    .route("/stats/client/{ip}", get(get_client))
    .fallback(|| async { StatusCode::NOT_FOUND });   // /api/typo -> 404, not index.html

let app = Router::new()
    .route("/healthz", get(healthz))
    .nest("/api", api)
    .fallback(assets::handler)                        // SPA, from rust-embed
    .layer(TraceLayer::new_for_http())
    .with_state(state);
```

`get_stats`: load the `Arc<Payload>`, compare `If-None-Match` → **304** when it matches,
else serve `gzip` or `json` by `Accept-Encoding`. An idle dashboard polling every 5s then
transfers a ~150-byte header and nothing else.

**Pre-gzipping once per tick beats `tower-http`'s `CompressionLayer`**, which would
re-compress the same 25 KB on every request — the same reasoning that motivated the
snapshot, applied consistently.

`get_client`: the on-demand drilldown, straight to SQLite, `?window=24h|7d|30d`. Validate
and bound `ip` (it goes into a parameterized query — but bound the length anyway).

`assets.rs`: `rust-embed` with `#[folder = "$CARGO_MANIFEST_DIR/../../frontend/dist"]`.
Serve `/assets/*` (vite hashes the filenames) with
`Cache-Control: public, max-age=31536000, immutable`, and `index.html` with `no-cache` —
without the second one a cached index pins old asset hashes and users see a broken page
after every deploy. Keep `debug-embed` off so `cargo run` reads from disk and
`npm run dev` hot-reloads without rebuilding Rust.

**Note**: `cargo build` now **fails if `frontend/dist` does not exist**. That is correct —
a UI-less binary is not shippable — but the error is confusing on a fresh clone. Commit
`stats/frontend/dist/.gitkeep` and document the `npm run build` prerequisite in
`stats/README.md`.

`/healthz`: 200 with `{status, last_tick_at, lag_seconds}`, **503** when
`lag_seconds > 60`.

**Verify**:
```bash
curl -s localhost:8081/api/stats | jq .kpis            # real numbers, matching sqlite
E=$(curl -sI localhost:8081/api/stats | grep -i etag | cut -d' ' -f2 | tr -d '\r')
curl -s -o /dev/null -w '%{http_code}\n' -H "If-None-Match: $E" localhost:8081/api/stats   # 304
curl -s -H 'Accept-Encoding: gzip' localhost:8081/api/stats --output - | file -   # gzip data
curl -s -o /dev/null -w '%{http_code}\n' localhost:8081/api/typo                  # 404
curl -s -o /dev/null -w '%{http_code}\n' localhost:8081/some/spa/route            # 200 (index)
curl -s localhost:8081/healthz | jq .
```

**Commit**: `task(T3): axum router, snapshot endpoint, embedded assets`

---

### T4: Container

**Files**: `stats/Dockerfile`, `stats/.dockerignore`

**What to do**: three stages — `node:22-alpine` (vite build) → `rust:1-alpine` +
`musl-dev` (cargo build, with the frontend `dist` copied in for `rust-embed`) →
`alpine:3.20` runtime with just the static binary.

**musl/alpine over glibc/debian-slim**, deliberately: ~15 MB vs ~90 MB, no runtime deps at
all, and — the reason that actually matters — a fully static binary needs no NSS lookup, so
running as an arbitrary `--user 1000:1000` with no `/etc/passwd` entry is a non-issue. The
two standard musl objections don't apply: the allocator handles one 5s tick of short-lived
strings, and this service makes **zero** outbound network calls so musl's resolver never
runs. Runtime is `alpine` rather than `scratch` so there's a busybox `wget` for the
HEALTHCHECK (matching `proxy/Dockerfile`'s convention) and a shell for debugging.

Use BuildKit cache mounts for the cargo registry and target dir — `rusqlite` bundled
compiles the SQLite amalgamation and you do not want that on every build.

`ENV TZ=UTC` in both containers, so the human `/dev/stdout` log and the dated log filename
agree. `USER 1000:1000` numeric, not a named user — a named user only exists for one uid,
and `./run` overrides with `--user ${APP_UID}:${APP_GID}` anyway.

**Verify**:
```bash
./build stats
docker images iphands/pkgcache-stats --format '{{.Size}}'     # ~15 MB
docker run --rm -v /tmp/pkgcache-test/stats:/data -p 8081:8081 iphands/pkgcache-stats &
curl -f localhost:8081/healthz
docker inspect --format '{{.State.Health.Status}}' <id>       # healthy
```

**Commit**: `task(T4): multi-stage proxy/Dockerfile, static musl binary`

---

### T5: Wire into `./run`, and the permission model

**Files**: `run`, `README.md`

**What to do**: `./run stats` launches the container detached with:
```
--user "${APP_UID}:${APP_GID}"
-p "${STATS_PORT}:8081"
-v "${LOGS_DIR}:/logs"          # rw -- the service prunes consumed files
-v "${STATS_DIR}:/data"         # rw -- stats.sqlite, .ingest.lock, labels.json
-v "${CACHE_DIR}:/cache:ro"     # ro -- statvfs + pkg/ size only
```
No shared network, no `--link`, no pod.

**Keep `./run` and `scripts/noir/create-stats.sh` in agreement.** `./run` is the dev-machine
path (docker, `/tmp/pkgcache-test/{data,logs,frontend}`); `scripts/noir/` is the host recipe
(podman, `/main/docker/cache/{data,logs,frontend}`, containers `cacher` and
`cacher-stats`). Same mounts, same uid, same semantics — if a task changes one, it changes
both in the same commit, or the live deploy silently diverges from what was tested.

**The thing that will actually bite:** both containers must run as the same
`APP_UID:APP_GID` **and be launched with the same userns flags**. Launch one with
`--userns=keep-id` and the other without and nginx writes logs the stats container cannot
read — and **the failure mode is a dashboard of silent zeros, not an error**. `./run all`
must pass identical `EXTRA_ARGS`/userns handling to both, and the README must say so.
Plan 03's loud EACCES ERROR plus `ingest.logs_readable` in the payload are the safety net.

Document the whole stats feature in `README.md`: what it shows, the port, the mounts, the
opt-in `:ro` cache mount, and how to verify it.

**Verify**:
```bash
shellcheck run
CACHE_DIR=/tmp/pkgcache-test/data ./run all
docker ps            # both containers Up and healthy
curl -f localhost:8081/healthz
curl -s localhost:8081/api/stats | jq '.ingest.logs_readable'   # true
# then the negative case, deliberately:
#   chmod 700 the logs dir as another uid -> logs_readable false + a loud ERROR in logs
```

**Commit**: `task(T5): run stats container; document the shared-uid requirement`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `stats/crates/stats/src/snapshot.rs` | payload build, pre-serialize, pre-gzip, ETag | P0 |
| `stats/crates/stats/src/api.rs` | router, 304 handling, drilldown | P0 |
| `stats/Dockerfile` | 3-stage musl build | P0 |
| `run` | stats container, mounts, matching uid/userns | P0 |
| `stats/frontend/src/lib/fixture.json` | frozen schema for Plan 05 | P0 |
| `stats/crates/stats/src/assets.rs` | rust-embed + cache headers | P1 |
| `README.md`, `stats/README.md` | user-facing docs | P1 |

---

## Open Questions / Risks

1. **Payload size at scale.** 10 clients is fine; 50 clients × 10 top-packages is not
   obviously fine. — *Mitigation:* measure in T2 and record it; the top-N bounds are the
   knob if it grows.
2. **`cargo build` fails without `frontend/dist`.** Confusing on a fresh clone. —
   *Mitigation:* `.gitkeep` + `stats/README.md` + a `just` recipe.
3. **A 5s poll with no ETag support would re-transfer 25 KB forever.** — *Mitigation:*
   the 304 path is in the T3 checklist; verify it rather than assuming React Query sends
   `If-None-Match` (it does, via `fetch`, but confirm).
4. **Three mounts weaken the "one purpose each" story slightly.** — *Mitigation:* each is
   scoped: `/logs` rw only so consumed files can be pruned (nothing else can do it safely),
   `/data` is the service's own, `/cache` is **read-only** and touched only by `statvfs`
   and a size walk. Never read package *content* through `/cache`.
5. **Alpine/musl build times** with `rusqlite` bundled. — *Mitigation:* BuildKit cache
   mounts; measure and record the cold and warm build times.
6. **`/healthz` 503 could flap** if a tick occasionally takes > 60s under heavy backlog. —
   *Mitigation:* the 16 MiB chunk cap bounds tick duration; if it flaps, raise the
   threshold rather than removing the check.

---

## Verification Checklist

- [ ] T1: ratios serialize as `null`, not `0.0`, when the denominator is zero
- [ ] T1: `bytes_saved` is never negative; equals `served − upstream` on real data
- [ ] T1: snapshot rebuilds every tick even with zero new lines
- [ ] T1: rebuild duration is single-digit ms on real data (logged at `debug`)
- [ ] T2: `fixture.json` deserializes into the payload type in a real test
- [ ] T2: fixture includes zero-traffic client, IPv6 client, `parse_errors > 0`, `logs_readable: false`
- [ ] T3: `/api/stats` numbers match `sqlite3` for the same window
- [ ] T3: repeat request with `If-None-Match` returns **304**
- [ ] T3: `Accept-Encoding: gzip` returns the pre-gzipped body
- [ ] T3: `/api/typo` → 404 (**not** index.html); `/some/spa/route` → 200 index.html
- [ ] T3: `index.html` served `no-cache`; `/assets/*` served `immutable`
- [ ] T3: `/healthz` → 503 when the ingest task is stopped, 200 when idle-but-alive
- [ ] T4: image ≈ 15 MB; runs as `--user 1000:1000`; HEALTHCHECK reports healthy
- [ ] T5: `./run` and `scripts/noir/create-stats.sh` specify the same mounts, uid and ports
- [ ] T5: `shellcheck run` clean; `./run all` brings up both containers
- [ ] T5: `logs_readable` is `true` normally, and `false` **with a loud ERROR** when the
      logs dir is unreadable (test the negative case deliberately)
- [ ] All: `README.md` documents the stats service in the same commit as the behavior
- [ ] All: findings harvested into `distilled.md` / `pitfalls.md` (Rule D)
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
