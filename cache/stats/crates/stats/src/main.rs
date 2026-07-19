//! pkgcache-stats: reads nginx's TSV access log, aggregates into SQLite, and
//! (from Plan 04) serves a dashboard.

use std::time::{SystemTime, UNIX_EPOCH};

pub mod db;
pub mod tail;

/// Current unix time in seconds. Kept in one place so tests and the DB layer
/// agree, and so there is exactly one call site to stub if that is ever needed.
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn main() {
    println!("pkgcache-stats (ingest core; CLI arrives in T5)");
}
