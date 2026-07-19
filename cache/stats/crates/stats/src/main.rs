//! pkgcache-stats: reads nginx's TSV access log, aggregates into SQLite, and
//! serves a dashboard.
//!
//! Two modes:
//!
//! ```text
//! pkgcache-stats --once    ingest whatever is available, print a summary, exit
//! pkgcache-stats           the tick loop (Plan 04 adds the HTTP server)
//! ```
//!
//! `--once` is not scaffolding. "Did the reader actually see this line?" is a
//! question worth being able to answer for the life of the service, and it is
//! what makes the strongest verification in this project possible: summing the
//! same log file two ways, by `sqlite3` and by `awk`, and requiring the answers
//! to match to the byte.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

pub mod config;
pub mod db;
pub mod tail;

/// Current unix time in seconds. One call site, so tests and the DB layer agree.
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let once = std::env::args().any(|a| a == "--once");
    let cfg = config::Config::from_env();

    std::fs::create_dir_all(&cfg.data_dir)
        .with_context(|| format!("create data dir {}", cfg.data_dir.display()))?;

    // Single-writer, before anything touches the database.
    let _lock = tail::acquire_writer_lock(&cfg.lock_path())?;

    let mut conn = db::open(&cfg.db_path(), cfg.wal)?;
    tracing::info!(
        logs = %cfg.logs_dir.display(),
        db = %cfg.db_path().display(),
        "pkgcache-stats starting"
    );

    if once {
        let report = tail::tick(&mut conn, &cfg.logs_dir)?;
        print_summary(&conn, &report)?;
        return Ok(());
    }

    tracing::info!("tick loop arrives with T6; use --once for now");
    Ok(())
}

/// Human-readable summary of a `--once` run, plus the lifetime totals.
///
/// Printed to stdout (not the tracing log) so it can be piped and diffed.
fn print_summary(conn: &rusqlite::Connection, r: &tail::TickReport) -> Result<()> {
    println!("files seen       {}", r.files_seen);
    println!("files advanced   {}", r.files_advanced);
    println!("bytes read       {}", r.bytes_read);
    println!("lines ingested   {}", r.lines);
    println!("parse errors     {}", r.parse_errors);
    println!("logs readable    {}", r.logs_readable);
    println!("--- lifetime totals ---");
    for k in db::TOTAL_KEYS {
        println!("{k:<24} {}", db::total(conn, k)?);
    }
    if !r.logs_readable {
        // Loud, because the alternative symptom is a dashboard of silent zeros.
        eprintln!(
            "\nWARNING: the log directory could not be read. nginx and this \
             service must run as the same uid:gid, launched the same way."
        );
    }
    Ok(())
}
