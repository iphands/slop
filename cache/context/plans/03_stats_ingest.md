# Plan 03 — Stats: Rust Ingest Core (log reader → SQLite)

> **Status**: pending
> **Created**: 2026-07-18
> **Depends on**: Plan 02 (the proxy emits 9-field TSV lines)
> **Goal**: `pkgcache-stats --once` reads the proxy's access logs, aggregates them, and writes numbers into `stats.sqlite` that match `awk` over the same file byte for byte — with no HTTP anywhere.
> **Agent**: implementation agent

---

> **Before writing any config, re-read `context/plans/RULES.md` in full** — especially
> Rule A (prove it runs and caches; there is no compiler here).
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Build the `stats/` Rust workspace and the log-ingest half of the stats service —
parsing, classification, aggregation, SQLite persistence, and crash-safe checkpointing.
**No HTTP, no frontend, no container.**

**Deliverables**:
1. `stats/` workspace: `crates/ingest` (pure, fully unit-tested) + `crates/stats` (binary)
2. SQLite schema with additive UPSERTs, WAL, retention and pruning
3. Crash-safe, idempotent tail with `(filename, inode, offset)` checkpointing
4. `pkgcache-stats --once` — ingest available logs, print a summary, exit
5. `pkgcache-stats` (default) — the 5s tick loop with hourly pruning
6. The first real test suite in this repo

**Estimated effort**: Medium (1 day)

---

## Context

Plan 02 leaves dated TSV logs on disk. This plan turns them into numbers. Plan 04 serves
those numbers over HTTP; Plan 05 draws them. The split exists because ingest and HTTP fail
in completely unrelated ways, and because ingest has a **uniquely strong verification
gate** available before any HTTP code exists: the same log file summed two ways, by
`sqlite3` and by `awk`, must agree to the byte.

### Why two crates

`RULES.md` opens by noting this project has no compiler and that Rule A exists to
compensate. Rust changes that for the first time — but only for the parts with no I/O.
`crates/ingest` is deliberately dependency-free (no tokio, no rusqlite, no filesystem): it
holds line parsing, URI classification, chunk splitting, and aggregation, so
`cargo test -p pkgcache-ingest` runs in under a second with no fixtures and no database.
That is the entire justification. Do not split further.

### The correctness invariant — everything below serves this sentence

> **Aggregate increments and the checkpoint offset commit in the same SQLite transaction,
> and there is exactly one writer process.**

Consequences, which is why it's worth stating as an invariant rather than a set of rules:

- Crash **before** COMMIT → aggregates *and* offset roll back together → the same bytes are
  re-read → the same additive UPSERTs are applied. Idempotent.
- Crash **after** COMMIT → offsets are durable → those bytes are never re-read. No
  double-count.
- There is no third state. Durability becomes an optimization rather than a requirement.

Single-writer is enforced with `flock(LOCK_EX|LOCK_NB)` on `/data/db/.ingest.lock` at
startup; on failure log an ERROR naming the file and **exit 1**. Two processes would each
read `offset=N`, each parse the same bytes, and each apply `+delta` — a silent 2× on every
number. **SQLite's own locking does not prevent this**; it serializes the writes, it does
not deduplicate the intent.

### Why aggregate on ingest rather than storing raw events

30 days of every request is millions of rows to answer questions that are all aggregates.
Hourly buckets keyed by `(hour, client_ip, repo, kind)` top out near 65k rows for 30 days
in the worst case and a few hundred realistically. A daily rollup table is *also* rejected:
it would add a second write path, a "has this hour been rolled up" state machine, and a new
double-counting hazard, to save single-digit megabytes.

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Log line format | 9 TSV fields, `$request_uri` last | Plan 02 T4 |
| Timestamp | field 1, `$msec`, epoch seconds with ms | Plan 02 |
| `$upstream_bytes_received` | field 6, `-` on a cache HIT | Plan 02 |
| `$upstream_cache_status` | field 7; `-` or empty on non-proxied locations | Plan 02 |
| Cache-status values | `HIT MISS EXPIRED STALE UPDATING REVALIDATED BYPASS` | nginx docs |
| Host FS supports WAL | *(read the Plan 02 tracker — if not local, use `journal_mode=TRUNCATE`)* | Plan 02 |
| `$upstream_bytes_received` can EXCEED `$body_bytes_sent` on a MISS | it includes response headers | **verified live 2026-07-18** |
| `$request_uri` keeps percent-encoding | `usbmuxd-1.1.1%5e2025….rpm` | verified live + production log |
| Container engine | **docker** to verify here; **podman** on `noir.lan` (no docker there) | 2026-07-18 |
| First database in the slop family | no rusqlite/sqlx/sled in qbots or qctrl, not even transitively | explored 2026-07-18 |
| House style | `#[cfg(test)] mod tests` in-file; sentence-style test names; pure fns extracted to be testable | qbots/qctrl, 2026-07-18 |

---

## Step-by-Step Tasks

### T1: Workspace scaffold

**Files**: `stats/Cargo.toml`, `stats/rust-toolchain.toml`, `stats/.gitignore`,
`stats/README.md`, `stats/crates/{ingest,stats}/Cargo.toml`

**What to do**:

```
stats/
├── Cargo.toml              # [workspace] members = ["crates/ingest", "crates/stats"]
├── Cargo.lock              # committed (binary)
├── rust-toolchain.toml     # channel = "stable", components = ["rustfmt", "clippy"]
├── .gitignore              # /target/  /target-*/  **/*.rs.bk
├── README.md               # the dev loop
└── crates/
    ├── ingest/             # pkgcache-ingest — PURE. no fs, no sqlite, no tokio.
    └── stats/              # pkgcache-stats — the binary
```

Follow qbots' workspace shape (`[workspace.package] edition = "2021"` + `edition.workspace = true`),
not qctrl's per-crate duplication.

`crates/ingest` deps: `serde` + `thiserror` only. Adding anything else to this crate needs
a justification in the commit message.

`crates/stats` deps: `pkgcache-ingest`, `rusqlite = { version = "0.32", features = ["bundled"] }`,
`tokio` with **explicit** features (`rt-multi-thread`, `macros`, `signal`, `time`) not `full`,
`tracing`, `tracing-subscriber`, `anyhow`, `fd-lock`, `serde`, `serde_json`.

`rusqlite` **bundled**: it vendors the SQLite amalgamation, so the runtime image needs no
`libsqlite3` and the version is pinned to the source tree rather than to whatever the base
image ships — which is what guarantees `STRICT` tables, UPSERT, and
`PRAGMA incremental_vacuum` are present. Cost is `musl-dev` in the builder and ~30s of C
compile. Note this in the manifest as a comment; it's the first DB in the family and there
is no house precedent to point at.

**Verify**: `cd stats && cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt --check`

**Commit**: `task(T1): stats workspace scaffold`

---

### T2: The pure ingest crate

**Files**: `stats/crates/ingest/src/{lib,chunk,line,classify,agg}.rs`

**What to do**: four modules, each with in-file `#[cfg(test)] mod tests` and sentence-style
test names (`a_line_with_eight_fields_is_a_parse_error`).

**`chunk.rs`** — never consume a partial line:
```rust
/// Returns (complete_lines, bytes_consumed). Bytes after the LAST '\n' are a
/// partial line and are NOT consumed — the writer may still be mid-write.
pub fn split_complete_lines(buf: &[u8]) -> (&[u8], usize) {
    match buf.iter().rposition(|&b| b == b'\n') {
        Some(i) => (&buf[..=i], i + 1),
        None    => (&[], 0),
    }
}
```

**`line.rs`** — `parse_line(&str) -> Result<Event<'_>, ParseError>`, borrowing not
allocating. Reject and count as a parse error: wrong field count, unparseable `$msec`, or a
`msec` outside `[now − 90d, now + 1d]` (an NTP-less box booting at epoch 0 must not create
an orphan bucket). `-` and `""` both parse to zero / `CacheClass::None`.

```rust
pub enum CacheClass { Hit, Miss, Bypass, None }
// HIT|REVALIDATED|STALE|UPDATING => Hit    MISS|EXPIRED => Miss
// BYPASS => Bypass                          "-"|""       => None
```

**`classify.rs`** — `classify(uri) -> (repo, Kind, path)`. `repo` is the first path
segment; `Kind::Package` iff the path ends `.deb|.udeb|.rpm`, else `Metadata`, except
`Kind::Other` when there is no leading `/` or the first segment is empty. Strip at `?`,
collapse `//`, truncate the path at 512 chars with a marker. **Test the Fedora case
explicitly**: `/fedora/…/foo.rpm` → repo `fedora`, and add a test asserting that a
`/pub/fedora/…` input is *not* what we expect to see, with a comment pointing at
`pitfalls.md` — so anyone who "fixes" nginx back to `$uri` gets a red test rather than a
wrong dashboard.

**`agg.rs`** — a `Batch` accumulating into two `HashMap`s plus a totals delta:
```rust
impl Batch {
    pub fn add(&mut self, ev: &Event<'_>);
    pub fn drain(&mut self) -> (Vec<HourRow>, Vec<PathRow>, Totals);
}
```
`hour_ts = floor(msec/3600)*3600`, `day_ts = floor(msec/86400)*86400`, both UTC.

**`bytes_saved` is not stored** — it is derived at query time so it can never disagree with
its inputs. The formula, **corrected against live data** (see Plan 02 Key Facts):

```
bytes_saved = Σ over {HIT, REVALIDATED, STALE, UPDATING} of
                  max(0, body_bytes_sent − upstream_bytes_received)
```

Do **not** use a whole-window `Σ served − Σ upstream`. Measured live 2026-07-18: a MISS
logged `body_bytes_sent=5966` with `upstream_bytes_received=6499` — **upstream exceeds
served**, because `$upstream_bytes_received` includes response headers and
`$body_bytes_sent` does not. A whole-window subtraction therefore charges every MISS's
header overhead against the savings, understating them systematically and going negative
on a quiet day.

Subtracting *within the hit class* gets all three cases right: `HIT` has `upstream = 0` so
the full body counts; `REVALIDATED` correctly nets out its ~300-byte 304 round-trip; `MISS`
contributes exactly 0 rather than a negative number.

To make this computable, `agg_hour` needs `bytes_upstream` **split by class** — store
`bytes_upstream_hit` alongside `bytes_upstream` rather than a single column, or the
hit-class subtraction cannot be reconstructed at query time.

Cardinality guards live here: **no `agg_path` row when `status >= 400`** (a 404 storm from
a scanner is the realistic blowup shape and has zero analytical value), and a cap of 5,000
distinct paths per `(day, client)` folding into `path = "(other)"`.

**Verify**:
```bash
cargo test -p pkgcache-ingest        # all green, < 1s, no fixtures
cargo clippy --all-targets -- -D warnings
```
Tests must cover: 8/9/10-field lines, `-` in fields 6 and 7, every cache-status value, a
tab escaped as `\x09` inside the URI, `.rpm`/`.deb`/`.udeb`/metadata/`Other` classification,
query-string stripping, the 512-char cap, an out-of-range `msec`, an empty buffer, a buffer
with no newline, and a buffer ending mid-line.

**Commit**: `task(T2): pure ingest crate — parse, classify, aggregate`

---

### T3: SQLite schema and store

**File**: `stats/crates/stats/src/db.rs`

**What to do**: PRAGMAs first — **`auto_vacuum = INCREMENTAL` must be set before the first
table exists** or it cannot be changed without a full `VACUUM`.

```sql
PRAGMA auto_vacuum = INCREMENTAL;
PRAGMA journal_mode = WAL;      -- see Plan 02 tracker: NOT viable on NFS/CIFS
PRAGMA synchronous = FULL;      -- one fsync per 5s tick; we write once per tick, so buy durability
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
```

Tables (all `STRICT`):

- **`meta(key, value)`** — `schema_version`, `created_at`.
- **`ingest_state(filename PK, inode, offset, size_seen, updated_at)`** — plus
  `CREATE INDEX ingest_state_inode ON ingest_state(inode)`. That index is what lets a
  *renamed* file adopt its existing offset, which kills the "someone ran logrotate and
  every number doubled" class outright.
- **`agg_hour(hour_ts, client_ip, repo, kind, req_hit, req_miss, req_bypass, req_none,
  req_err, bytes_hit, bytes_miss, bytes_bypass, bytes_none, bytes_upstream,
  bytes_upstream_hit, time_sum_ms)`**
  — `PRIMARY KEY (hour_ts, client_ip, repo, kind)`, `WITHOUT ROWID` (rows are ~120 bytes,
  comfortably under the ~1/20-page guidance). Index `(client_ip, hour_ts)` for the client
  table.
- **`agg_path(day_ts, client_ip, repo, kind, path, reqs, req_hit, bytes, bytes_hit,
  last_ts)`** — **day** grain, and a **rowid table**, because `path` can be 512 bytes which
  is far too wide for `WITHOUT ROWID`. Unique index on the PK tuple as the UPSERT target,
  plus `(day_ts, kind, bytes DESC)` for global top-N and `(client_ip, day_ts, bytes DESC)`
  for the drilldown.
- **`totals(key, value)`** — lifetime counters, **never pruned**, so the headline "saved
  since forever" survives the 30-day window.
- **`client_label(client_ip PK, label)`** — hand-maintained friendly names.

All writes are additive UPSERTs (`ON CONFLICT … DO UPDATE SET col = col + excluded.col`).
That, plus the single transaction, is what makes a re-ingest a re-count rather than a
mis-count.

Startup **`PRAGMA quick_check`**; on failure rename to `stats.sqlite.corrupt.<epoch>`, log
ERROR, and start fresh. **Put a comment saying this is correct *because it is a stats DB***
— losing regenerable history is a nuisance, a crash-looping container is an outage — and
that the pattern must not be copied somewhere holding data you can't regenerate.

**Verify**:
```bash
cargo test -p pkgcache-stats db::         # schema creates; UPSERT accumulates; quick_check path
sqlite3 /tmp/t.sqlite '.schema' | grep -c STRICT
sqlite3 /tmp/t.sqlite 'PRAGMA journal_mode; PRAGMA auto_vacuum;'   # wal | 2
```

**Commit**: `task(T3): sqlite schema, PRAGMAs, additive UPSERTs`

---

### T4: The tail — discovery, checkpointing, transaction

**File**: `stats/crates/stats/src/tail.rs`

**What to do**: one tick, in order.

1. **flock** `/data/db/.ingest.lock` at startup (`LOCK_EX|LOCK_NB`); ERROR + exit 1 on
   failure.
2. **Discover.** `readdir("/data/logs")`, keep `^access-(\d{4}-\d{2}-\d{2}|nodate)\.log$`,
   sort ascending — ISO dates sort chronologically and `nodate` sorts last. Oldest first.
   Worth a comment: ordering is *tidiness, not correctness* — every line carries its own
   `$msec`, so buckets are right regardless of read order. That removes the temptation to
   build ordering machinery.
3. **Per file, pick the start offset:**

   | Condition | Action |
   |---|---|
   | `open` → ENOENT | skip silently (pruned between readdir and open) |
   | no `ingest_state` row for the name | look up by **inode** → adopt offset + rewrite name; else 0 |
   | `st.inode != row.inode` | WARN "log file replaced"; offset 0 |
   | `st.size < row.offset` | WARN "log file truncated"; offset 0 |
   | otherwise | `row.offset` |
   | `size == offset` | nothing new; next file |

4. **Read a bounded chunk**: `pread(fd, buf, min(size - offset, 16 MiB), offset)`. The cap
   bounds RSS; the remainder arrives next tick. A week of backlog drains at 16 MiB/tick
   without a memory spike.
5. **Split** with `split_complete_lines`. If `consumed == 0` **and** the buffer is at the
   16 MiB cap, a single line has no newline in 16 MiB: log ERROR, count one parse error,
   and **consume the whole cap**. Without this escape hatch one corrupt byte range stalls
   that file and every later file forever.
6. **Parse and accumulate** across all files into one `Batch`.
7. **One transaction** — `BEGIN IMMEDIATE` (take the write lock up front rather than
   discovering an upgrade failure at COMMIT):
   ```
   BEGIN IMMEDIATE;
     UPSERT each agg_hour delta
     UPSERT each agg_path delta
     UPDATE totals   SET value = value + ?
     UPSERT ingest_state for every advanced file
   COMMIT;
   ```
8. **Log a loud ERROR if `/data/logs` exists but `readdir` returns EACCES**, and track a
   `logs_readable` flag. This is the single highest-value line of instrumentation in the
   whole subsystem: a uid mismatch between the two containers produces **a dashboard of
   silent zeros with no error anywhere**, and this converts it into one sentence on screen.

**Verify**:
```bash
cargo test -p pkgcache-stats tail::   # inode change, truncation, rename-adoption, partial line
# idempotency, for real:
./pkgcache-stats --once && sqlite3 db 'SELECT sum(bytes_hit) FROM agg_hour' > /tmp/a
./pkgcache-stats --once && sqlite3 db 'SELECT sum(bytes_hit) FROM agg_hour' > /tmp/b
diff /tmp/a /tmp/b     # identical — a second pass over the same bytes changes nothing
```

**Commit**: `task(T4): crash-safe log tail with inode/offset checkpointing`

---

### T5: `--once` mode and the awk cross-check

**Files**: `stats/crates/stats/src/{main,config}.rs`

**What to do**: `pkgcache-stats --once` ingests everything available, prints a summary
(lines, parse errors, files advanced, bytes served/upstream/saved, per-kind totals), and
exits 0. This is **not scaffolding** — "did the reader actually see this line?" is a
question you will ask for the life of the service.

Config from env with the `${VAR:-default}` discipline the shell scripts already use:
`PKGCACHE_DATA=/data`, `PKGCACHE_TICK_SECONDS=5`, `PKGCACHE_LOG_RETENTION_DAYS=3`,
`PKGCACHE_DB_RETENTION_DAYS=30`, `RUST_LOG=info`.

Normalize `::ffff:1.2.3.4` → `1.2.3.4` here or in `classify` — `listen [::]:8080` is in the
proxy config, so a dual-stack client otherwise appears as two unrelated rows.
**No reverse DNS**: this service makes zero network calls by design. Friendly names come
from `/data/labels.json`, hot-reloaded each tick into `client_label`.

**Verify — this is the strongest gate in the project.** Against one real log file:
```bash
L=/tmp/pkgcache-test/stats/logs/access-2026-07-18.log
awk -F'\t' '{s+=$5} END {print s}' "$L"                            # bytes served
sqlite3 db 'SELECT sum(bytes_hit+bytes_miss+bytes_bypass+bytes_none) FROM agg_hour'
#   ^ these two MUST be equal

awk -F'\t' '$6 != "-" {s+=$6} END {print s}' "$L"                  # bytes upstream
sqlite3 db 'SELECT sum(bytes_upstream) FROM agg_hour'
awk -F'\t' 'END {print NR}' "$L"                                   # line count
sqlite3 db "SELECT value FROM totals WHERE key='lines_ingested'"
```
Any mismatch is a bug — not a rounding artifact, not a race. Do not proceed to Plan 04
until they agree exactly.

**Commit**: `task(T5): --once ingest mode + env config`

---

### T6: Tick loop, retention, pruning

**File**: `stats/crates/stats/src/main.rs`

**What to do**: a tokio interval loop at `PKGCACHE_TICK_SECONDS`, with
`MissedTickBehavior::Delay` (house convention — qctrl sets it on every interval), and a
SIGTERM/SIGINT handler that finishes the in-flight transaction and exits cleanly.

**Hourly**, not per tick:
- **Log pruning** — delete `access-YYYY-MM-DD.log` only when **all** of: older than
  `LOG_RETENTION_DAYS` (3); fully ingested (`offset >= size`); and **not** today's or
  yesterday's, unconditionally. That last condition is the safety margin for
  `open_log_file_cache valid=1m` — **delete a file nginx still holds an fd for and it keeps
  appending to an unreachable inode, silently losing every request.** If the
  fully-ingested check fails, WARN and retry next hour rather than deleting unread data.
- `access-nodate.log` is **never** pruned, and a non-empty one logs a WARN — it means the
  `$time_iso8601` map failed and something is structurally wrong upstream.
- Drop `ingest_state` rows whose file is gone from disk.
- `DELETE FROM agg_hour/agg_path` past `DB_RETENTION_DAYS`, then
  `PRAGMA incremental_vacuum(200)` — bounded, never a long stall. **Never `VACUUM`** (needs
  2× the DB free and locks the world).

**Verify**:
```bash
# fabricate old log files and confirm the policy
touch -d '10 days ago' $L/access-2026-07-08.log
./pkgcache-stats  # ... one hour, or force the prune path in a test
# assert: today's and yesterday's files still exist; a not-fully-ingested old file survives
cargo test -p pkgcache-stats prune::
```

**Commit**: `task(T6): tick loop, log pruning, DB retention`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `stats/crates/ingest/src/{chunk,line,classify,agg}.rs` | the pure, tested core | P0 |
| `stats/crates/stats/src/tail.rs` | checkpointing + the single transaction | P0 |
| `stats/crates/stats/src/db.rs` | schema, PRAGMAs, additive UPSERTs | P0 |
| `stats/crates/stats/src/main.rs` | `--once`, tick loop, pruning, flock | P0 |
| `stats/Cargo.toml`, `rust-toolchain.toml` | workspace | P1 |
| `stats/README.md` | dev loop | P1 |
| `context/distilled.md` | SQLite/ingest findings (Rule D) | P2 |

---

## Open Questions / Risks

1. **WAL may not be viable** if `/main/docker/cache/data` is network storage. — *Mitigation:*
   read the Plan 02 tracker's recorded `findmnt` result before writing `db.rs`; fall back to
   `journal_mode=TRUNCATE`.
2. **Two writers silently double every number.** — *Mitigation:* `flock` + exit 1. Test it
   by launching two `--once` runs concurrently and asserting the second exits non-zero.
3. **A uid mismatch produces silent zeros, not an error.** — *Mitigation:* T4's loud EACCES
   ERROR and the `logs_readable` flag, surfaced in Plan 04's payload.
4. **Cardinality blowup** from a scanner hitting thousands of distinct 404 URLs. —
   *Mitigation:* no `agg_path` rows for `status >= 400`; 5,000-path-per-client-day cap
   folding into `(other)`.
5. **HEAD requests** (`curl -sI` verification traffic, monitors) log `body_bytes_sent=0`
   with a real cache status, dragging the byte-ratio denominator down without contributing
   bytes. Easy to miss because it only distorts the metric *while you're testing it*. —
   *Mitigation:* method is field 3; store it or exclude HEAD from byte ratios, and decide
   here rather than in Plan 04.
6. **`--once` on a live log races the writer.** — *Mitigation:* by construction — the
   partial-line rule means an in-flight line is simply not consumed this pass.
7. **`rusqlite` bundled adds a C compile** to every clean build. — *Mitigation:* accepted;
   Plan 04's Dockerfile uses a BuildKit cache mount.

---

## Verification Checklist

- [ ] T1: `cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all clean
- [ ] T2: `cargo test -p pkgcache-ingest` green in < 1s with no fixtures and no DB
- [ ] T2: a `/pub/fedora/…` input has an explicit test documenting it as the *wrong* shape
- [ ] T3: schema is `STRICT`; `PRAGMA journal_mode` → `wal`; `auto_vacuum` → `2`
- [ ] T3: repeated UPSERT of the same key accumulates rather than replacing
- [ ] T4: unit tests cover inode change, truncation, rename-adoption, partial line, empty buffer
- [ ] T4: two `--once` runs over the same log produce **identical** sums (idempotency)
- [ ] T4: `kill -9` mid-ingest, re-run, sums still correct (no double count, no loss)
- [ ] T4: a second concurrent process exits 1 with the lock-file path in the message
- [ ] T5: **`sqlite3` sums == `awk` sums, exactly** — bytes served, bytes upstream, line count
- [ ] T5: `--once` summary prints parse errors, and a deliberately corrupted line increments it
- [ ] T5: `::ffff:` addresses collapse into their IPv4 row
- [ ] T6: today's and yesterday's logs are never pruned regardless of retention
- [ ] T6: a not-fully-ingested old file is kept and WARNed, not deleted
- [ ] T6: `access-nodate.log` is never pruned; non-empty triggers a WARN
- [ ] T6: SIGTERM completes the in-flight transaction and exits 0
- [ ] All: findings harvested into `distilled.md` / `pitfalls.md` (Rule D)
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
