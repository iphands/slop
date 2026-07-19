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

pub mod api;
pub mod config;
pub mod db;
pub mod prune;
pub mod snapshot;
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

    run_loop(conn, cfg)
}

/// Build the initial snapshot so the API has something to serve immediately.
fn initial_snapshot(conn: &rusqlite::Connection, cfg: &config::Config) -> api::Rendered {
    let ing = snapshot::Ingest {
        last_tick_at: now_secs(),
        logs_readable: true,
        ..Default::default()
    };
    let p = snapshot::build(conn, now_secs(), ing, cfg.cache_dir.as_deref())
        .unwrap_or_else(|e| panic!("initial snapshot: {e:#}"));
    api::Rendered::build(&p)
}

/// The tick loop: ingest every `tick_seconds`, prune hourly, exit cleanly.
#[tokio::main(flavor = "current_thread")]
async fn run_loop(mut conn: rusqlite::Connection, cfg: config::Config) -> Result<()> {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(cfg.tick_seconds));
    // House convention (qctrl sets this on every interval): after a slow tick,
    // delay rather than firing a burst to "catch up".
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let snap = api::Snapshot::new(initial_snapshot(&conn, &cfg));

    // The HTTP server shares the snapshot cell and gets its own connection for
    // the on-demand drilldown, so it never blocks the ingest writer.
    let http_conn = db::open(&cfg.db_path(), cfg.wal)?;
    let state = api::AppState {
        snapshot: snap.clone(),
        db: std::sync::Arc::new(std::sync::Mutex::new(http_conn)),
    };
    let bind = cfg.bind.clone();
    tokio::spawn(async move {
        match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => {
                tracing::info!(%bind, "dashboard listening");
                if let Err(e) = axum::serve(l, api::router(state)).await {
                    tracing::error!("http server stopped: {e}");
                }
            }
            Err(e) => tracing::error!(%bind, "cannot bind: {e}"),
        }
    });

    let mut last_readable = true;
    let mut last_prune = 0i64;
    let mut shutdown = std::pin::pin!(shutdown_signal());

    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = &mut shutdown => {
                // The in-flight transaction, if any, has already committed --
                // select! only cancels at an await point, and tick::run holds
                // no await. Nothing to drain.
                tracing::info!("shutdown signal received; exiting cleanly");
                return Ok(());
            }
        }

        match tail::tick(&mut conn, &cfg.logs_dir) {
            Ok(r) => {
                last_readable = r.logs_readable;
                if r.lines > 0 || r.parse_errors > 0 {
                    tracing::info!(
                        lines = r.lines,
                        parse_errors = r.parse_errors,
                        files = r.files_advanced,
                        bytes = r.bytes_read,
                        "ingested"
                    );
                }
            }
            // A failed tick must not kill the loop: the next one retries from
            // the same offsets, because nothing was committed.
            Err(e) => tracing::error!("ingest tick failed: {e:#}"),
        }

        // Rebuild UNCONDITIONALLY, even when nothing was ingested: otherwise an
        // idle cache freezes the rolling 24h window where traffic stopped.
        let ing = snapshot::Ingest {
            last_tick_at: now_secs(),
            lag_seconds: 0,
            files_tracked: db::tracked_filenames(&conn).map(|v| v.len()).unwrap_or(0),
            lines_ingested: db::total(&conn, "lines_ingested").unwrap_or(0),
            parse_errors: db::total(&conn, "parse_errors").unwrap_or(0),
            logs_readable: last_readable,
            db_bytes: db::db_bytes(&cfg.db_path()),
        };
        match snapshot::build(&conn, now_secs(), ing, cfg.cache_dir.as_deref()) {
            Ok(p) => snap.store(api::Rendered::build(&p)),
            Err(e) => tracing::error!("snapshot rebuild failed: {e:#}"),
        }

        let now = now_secs();
        if now - last_prune >= 3600 {
            last_prune = now;
            match prune::run(
                &mut conn,
                &cfg.logs_dir,
                now,
                cfg.log_retention_days,
                cfg.db_retention_days,
            ) {
                Ok(r)
                    if r.logs_deleted > 0 || r.hour_rows_deleted > 0 || r.path_rows_deleted > 0 =>
                {
                    tracing::info!(?r, "retention pass");
                }
                Ok(_) => {}
                Err(e) => tracing::error!("prune failed: {e:#}"),
            }
        }
    }
}

/// Resolve on SIGTERM (container stop) or SIGINT (Ctrl-C).
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("cannot install SIGTERM handler: {e}");
            return std::future::pending().await;
        }
    };
    let mut int = match signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(_) => return std::future::pending().await,
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv()  => {}
    }
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
