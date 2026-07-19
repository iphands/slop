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
| 4 | T4 crash-safe tail | pending | — | flock, inode/offset, single transaction |
| 5 | T5 `--once` + env config | pending | — | **the awk cross-check gate** |
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
