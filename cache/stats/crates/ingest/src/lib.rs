//! The pkgcache stats ingest core.
//!
//! **This crate is deliberately pure**: no filesystem, no SQLite, no tokio, and
//! only `serde` + `thiserror` as dependencies. Everything here is a function
//! from bytes or strings to values, which is what lets `cargo test -p
//! pkgcache-ingest` run in well under a second with no fixtures and no database.
//!
//! `context/plans/RULES.md` opens by noting that the proxy half of this repo has
//! no compiler, and that Rule A (build it, run it, prove MISS→HIT) exists to
//! compensate. This crate is the part that *does* have a compiler, so the logic
//! that can be tested properly lives here and the I/O lives next door in
//! `pkgcache-stats`.
//!
//! Pipeline:
//!
//! ```text
//! bytes ──chunk──> whole lines ──line──> Event ──classify──> (repo, kind, path)
//!                                          │                        │
//!                                          └────────agg─────────────┴──> rows
//! ```

pub mod agg;
pub mod chunk;
pub mod classify;
pub mod line;
pub mod pkgname;

pub use agg::{Batch, Drained, HourCounters, HourKey, PathCounters, PathKey, Totals};
pub use chunk::split_complete_lines;
pub use classify::{classify, Classified, Kind};
pub use line::{parse_line, parse_line_at, CacheClass, Event, ParseError};
pub use pkgname::{display_name, parse_path, PkgName};
