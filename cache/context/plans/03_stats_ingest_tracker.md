# Stats Ingest Core — Tracker

## Overview
- Status: 0% complete (0 of 6 tasks)
- Start date: *(not started)*
- Depends on: Plan 02 complete — the proxy must be emitting 9-field TSV logs, and the
  Plan 02 tracker must have the host filesystem type recorded (WAL viability).
- Test data: `/tmp/pkgcache-test/logs/access-*.log` produced by Plan 02's verification
- DB under test: `/tmp/pkgcache-test/frontend/stats.sqlite`

## Resume Instructions

Read Plan 03's Context section first — specifically the **correctness invariant**
(aggregates and offset commit in one transaction; exactly one writer). Every rule in T3
and T4 exists to serve that one sentence, and a change that violates it will pass all the
unit tests and silently corrupt totals in production.

**Before writing `db.rs`, read the Plan 02 tracker's Environment Facts table** for the
`findmnt` result. SQLite WAL does not work on NFS or CIFS; if the host cache dir is network
storage, use `journal_mode=TRUNCATE` instead and record that deviation below.

Task order matters: T2 (pure crate) is testable with no I/O at all and should be green
before any SQLite exists. T5's awk cross-check is the gate that must pass before Plan 04
starts — do not treat a near-match as a pass.

The `--once` binary from T5 is a permanent debugging asset, not scaffolding. Keep it
working after T6 adds the tick loop.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: workspace scaffold | `stats/Cargo.toml`, `rust-toolchain.toml` | pending | qbots-style `[workspace.package]`; ingest crate gets 2 deps only |
| 2 | T2: pure ingest crate | `crates/ingest/src/{chunk,line,classify,agg}.rs` | pending | first real test suite in this repo |
| 3 | T3: sqlite schema + store | `crates/stats/src/db.rs` | pending | `auto_vacuum` must be set before the first table |
| 4 | T4: crash-safe tail | `crates/stats/src/tail.rs` | pending | flock; inode/offset; single transaction |
| 5 | T5: `--once` + env config | `crates/stats/src/{main,config}.rs` | pending | **the awk cross-check gate** |
| 6 | T6: tick loop + pruning | `crates/stats/src/main.rs` | pending | never delete today's/yesterday's log |

## Decisions Pending Confirmation

| Question | Default if unanswered | Decide by |
|---|---|---|
| WAL or TRUNCATE journal mode? | WAL (assumes local FS) | T3 |
| Store HEAD requests, or exclude from byte ratios? | store `method`; exclude HEAD in Plan 04's queries | T5 |
| 5,000-path-per-client-day cap — right number? | 5,000, folding into `(other)` | T2 |

## Notes / Deviations

*(none yet — record anything the plan asserted that turns out to be wrong, bluntly. A
wrong Key Fact recorded honestly here is worth more than a clean-looking tracker.)*

## Progress log (live)

| # | Task | Status | Commit | Notes |
|---|------|--------|--------|-------|
| 1 | T1 workspace scaffold | **done** | `26619cfe5` | rustc 1.94.1; build/clippy/fmt clean |
| 2 | T2 pure ingest crate | **done** | `b42d33cbb` | 63 tests, 0.05s, no fixtures |
| 3 | T3 sqlite schema + store | **done** | `be803538a` + `431024d58` | 10 more tests; see the fix commit |
| 4 | T4 crash-safe tail | **done** | `e003a45e5` | 12 tests: rename adoption, truncation, replacement, partial line, lock |
| 5 | T5 `--once` + env config | **done** | `9f49a2b0c` + `8080e3fa1` | **GATE PASSED** — and found a real bug |
| 6 | T6 tick loop + pruning | pending | — | never delete today's/yesterday's log |

### Deviations so far

- **`be803538a` shipped with a false claim in its message** ("clippy clean" — it
  was not). Cause: `cargo clippy ... | tail -2 && ...` exits with *tail's*
  status, so a failing clippy returned 0 and the `&&` chain continued. Corrected
  forward in `431024d58`, per the append-only rule. **All verification now uses
  `set -o pipefail`.** This is the second instance of the same shape as the
  heredoc bug behind `c815c54e6` — a check whose failure did not propagate.
- Two bugs the tests caught, both in my *tests* rather than the code: `ParseError`
  derived `Eq` while holding an `f64`; and a day-bucket expectation of
  `1784376000`, which is not a multiple of 86400. Added an assertion that every
  bucket is an exact multiple of its width.
- `Drained` type alias added to `agg.rs` to satisfy `clippy::type_complexity`
  properly, replacing an `#[allow]`.

### Environment

- rustc/cargo **1.94.1** (gentoo). `/tmp` is tmpfs, so WAL is fine for dev tests;
  the live host's `findmnt -no FSTYPE /main/docker/cache` is **still unrecorded**
  and gates the WAL-vs-TRUNCATE choice in production.

### T5: the gate did its job

Against 16 real lines from the live proxy, `awk` and `sqlite3` disagreed by exactly one
line. Cause: **`$upstream_bytes_received` is not always a single number.** nginx logs one
value per upstream connection, comma/colon separated, when a request hits more than one
upstream (a `proxy_next_upstream` retry or an internal redirect):

```text
1784420365.440 … 404 … 300 … 0, 908 … MISS … /debian/does-not-exist.deb
```

The parser rejected the whole line, so every such request would have silently vanished.
Fixed with `sum_upstream_bytes()` (+2 regression tests, one using the exact line) and
recorded in `distilled.md`.

This is exactly the class of bug the 03/04 plan split exists to catch: nothing errored, the
numbers looked plausible, and only summing the same file two independent ways revealed it.

**Gate now passes on real data, all seven metrics to the byte** — lines 16/16,
bytes_served 10,480,680, bytes_upstream 3,514,280, requests 16/16, hit_requests 9/9,
errors 1/1, bytes_saved 6,970,131, parse_errors 0. Idempotency confirmed live (three
further `--once` runs ingest 0 lines, totals unchanged), as is the unreadable-log-dir path
(`logs readable false` + the loud same-uid warning).

### Next

**T6** — the tick loop, log pruning and DB retention — is the only task left in Plan 03.
The pruning rules are the delicate part: delete a log only when it is older than
`LOG_RETENTION_DAYS` **and** fully ingested (`offset >= size`) **and** not today's or
yesterday's, that last condition being the margin for nginx's `open_log_file_cache`, which
holds an fd for up to a minute after the last write. Unlink a file nginx still holds and it
appends to an unreachable inode, silently losing every request.
