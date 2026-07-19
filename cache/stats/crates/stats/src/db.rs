//! SQLite schema and store.
//!
//! # The correctness invariant
//!
//! > Aggregate increments and the checkpoint offset commit in the **same
//! > transaction**, and there is exactly one writer process.
//!
//! Everything here serves that sentence. Crash before COMMIT and both roll back,
//! so re-reading the same bytes applies the same additive deltas. Crash after
//! and the offsets are durable, so those bytes are never re-read. There is no
//! third state, which is what makes durability an optimisation rather than a
//! requirement.
//!
//! Every write is an **additive** UPSERT (`col = col + excluded.col`), so a
//! replay is a re-count rather than a mis-count.

use anyhow::{Context, Result};
use pkgcache_ingest::{HourCounters, HourKey, PathCounters, PathKey, Totals};
use rusqlite::{params, Connection, Transaction};

/// Bumped only for a breaking schema change; `meta.schema_version` records it.
pub const SCHEMA_VERSION: i64 = 1;

/// Open (creating if needed) the stats database at `path`.
///
/// `wal` should be false only when the data directory is on NFS/CIFS, where WAL
/// does not work. See the Plan 02 tracker for the host filesystem check.
pub fn open(path: &std::path::Path, wal: bool) -> Result<Connection> {
    let fresh = !path.exists();
    let mut conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;

    // auto_vacuum MUST be set before the first table exists; it cannot be
    // changed later without a full VACUUM.
    if fresh {
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    }
    conn.pragma_update(None, "journal_mode", if wal { "WAL" } else { "TRUNCATE" })?;
    // We write ONE transaction every few seconds, so a single fsync per tick is
    // cheap and buys real durability. The usual argument for NORMAL is write
    // throughput, which we do not need.
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;

    if let Err(e) = integrity_check(&conn) {
        // A stats database holds only regenerable aggregates, so starting fresh
        // beats crash-looping. DO NOT copy this pattern somewhere holding data
        // you cannot regenerate -- there, refusing to start is the right call.
        tracing::error!("integrity check failed ({e}); the database will be replaced");
        drop(conn);
        let corrupt = path.with_extension(format!("corrupt.{}", crate::now_secs()));
        std::fs::rename(path, &corrupt)
            .with_context(|| format!("rename corrupt db to {}", corrupt.display()))?;
        conn = Connection::open(path)?;
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        conn.pragma_update(None, "journal_mode", if wal { "WAL" } else { "TRUNCATE" })?;
        conn.pragma_update(None, "synchronous", "FULL")?;
    }

    migrate(&mut conn)?;
    Ok(conn)
}

fn integrity_check(conn: &Connection) -> Result<()> {
    let r: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    anyhow::ensure!(r == "ok", "quick_check returned {r}");
    Ok(())
}

fn migrate(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

-- One row per log file ever seen. `offset` is bytes fully parsed AND committed.
CREATE TABLE IF NOT EXISTS ingest_state (
    filename   TEXT    PRIMARY KEY,
    inode      INTEGER NOT NULL,
    offset     INTEGER NOT NULL,
    size_seen  INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- Lets a RENAMED file adopt its existing offset (inode survives, name does not),
-- which kills the "someone ran logrotate and every number doubled" class.
CREATE INDEX IF NOT EXISTS ingest_state_inode ON ingest_state(inode);

-- Fact table 1: everything except package identity.
-- Rows are ~120 bytes, comfortably under the WITHOUT ROWID guidance.
CREATE TABLE IF NOT EXISTS agg_hour (
    hour_ts            INTEGER NOT NULL,
    client_ip          TEXT    NOT NULL,
    repo               TEXT    NOT NULL,
    kind               TEXT    NOT NULL,

    req_hit            INTEGER NOT NULL DEFAULT 0,
    req_miss           INTEGER NOT NULL DEFAULT 0,
    req_bypass         INTEGER NOT NULL DEFAULT 0,
    req_none           INTEGER NOT NULL DEFAULT 0,
    req_err            INTEGER NOT NULL DEFAULT 0,
    req_head           INTEGER NOT NULL DEFAULT 0,

    bytes_hit          INTEGER NOT NULL DEFAULT 0,
    bytes_miss         INTEGER NOT NULL DEFAULT 0,
    bytes_bypass       INTEGER NOT NULL DEFAULT 0,
    bytes_none         INTEGER NOT NULL DEFAULT 0,

    bytes_upstream     INTEGER NOT NULL DEFAULT 0,
    -- Upstream bytes on HIT-class responses only. Separate from bytes_upstream
    -- because bytes_saved is a hit-class-only subtraction; with one combined
    -- column the formula cannot be reconstructed at query time.
    bytes_upstream_hit INTEGER NOT NULL DEFAULT 0,

    time_sum_ms        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (hour_ts, client_ip, repo, kind)
) STRICT, WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS agg_hour_client ON agg_hour(client_ip, hour_ts);

-- Fact table 2: package/path identity, at DAY grain.
-- A rowid table, not WITHOUT ROWID: `path` can be 512 bytes, far too wide.
CREATE TABLE IF NOT EXISTS agg_path (
    day_ts             INTEGER NOT NULL,
    client_ip          TEXT    NOT NULL,
    repo               TEXT    NOT NULL,
    kind               TEXT    NOT NULL,
    path               TEXT    NOT NULL,

    reqs               INTEGER NOT NULL DEFAULT 0,
    req_hit            INTEGER NOT NULL DEFAULT 0,
    bytes              INTEGER NOT NULL DEFAULT 0,
    bytes_hit          INTEGER NOT NULL DEFAULT 0,
    bytes_upstream_hit INTEGER NOT NULL DEFAULT 0,
    last_ts            INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS agg_path_pk
    ON agg_path(day_ts, client_ip, repo, kind, path);
CREATE INDEX IF NOT EXISTS agg_path_top    ON agg_path(day_ts, kind, bytes DESC);
CREATE INDEX IF NOT EXISTS agg_path_client ON agg_path(client_ip, day_ts, bytes DESC);

-- Lifetime counters. NEVER pruned, so "saved since forever" survives the
-- 30-day retention window.
CREATE TABLE IF NOT EXISTS totals (
    key   TEXT PRIMARY KEY,
    value INTEGER NOT NULL
) STRICT;

-- Hand-maintained friendly names for client IPs. No reverse DNS: this service
-- makes zero outbound network calls by design.
CREATE TABLE IF NOT EXISTS client_label (
    client_ip TEXT PRIMARY KEY,
    label     TEXT NOT NULL
) STRICT;
"#,
    )
    .context("create schema")?;

    conn.execute(
        "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SCHEMA_VERSION.to_string()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO meta(key, value) VALUES ('created_at', ?1)",
        params![crate::now_secs().to_string()],
    )?;
    Ok(())
}

/// Create the schema on an in-memory connection. Test-only: production always
/// goes through [`open`], which also sets the PRAGMAs and runs the integrity
/// check.
#[cfg(test)]
pub fn migrate_for_tests(conn: &mut Connection) {
    migrate(conn).expect("migrate test db");
}

/// Every lifetime counter we track, so callers cannot typo a key.
pub const TOTAL_KEYS: &[&str] = &[
    "reqs",
    "bytes_served",
    "bytes_upstream",
    "bytes_saved",
    "reqs_package",
    "reqs_metadata",
    "bytes_served_package",
    "bytes_served_metadata",
    "lines_ingested",
    "parse_errors",
    "decode_failures",
];

/// Apply one tick's aggregates AND the advanced checkpoints in a single
/// transaction. This function *is* the correctness invariant.
///
/// `BEGIN IMMEDIATE` takes the write lock up front rather than discovering an
/// upgrade failure at COMMIT.
pub fn commit_batch(
    conn: &mut Connection,
    hours: &[(HourKey, HourCounters)],
    paths: &[(PathKey, PathCounters)],
    totals: &Totals,
    checkpoints: &[Checkpoint],
) -> Result<()> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    write_hours(&tx, hours)?;
    write_paths(&tx, paths)?;
    write_totals(&tx, totals)?;
    write_checkpoints(&tx, checkpoints)?;
    tx.commit().context("commit batch")?;
    Ok(())
}

fn write_hours(tx: &Transaction<'_>, rows: &[(HourKey, HourCounters)]) -> Result<()> {
    let mut st = tx.prepare_cached(
        r#"
INSERT INTO agg_hour (hour_ts, client_ip, repo, kind,
    req_hit, req_miss, req_bypass, req_none, req_err, req_head,
    bytes_hit, bytes_miss, bytes_bypass, bytes_none,
    bytes_upstream, bytes_upstream_hit, time_sum_ms)
VALUES (?1,?2,?3,?4, ?5,?6,?7,?8,?9,?10, ?11,?12,?13,?14, ?15,?16,?17)
ON CONFLICT(hour_ts, client_ip, repo, kind) DO UPDATE SET
    req_hit            = req_hit            + excluded.req_hit,
    req_miss           = req_miss           + excluded.req_miss,
    req_bypass         = req_bypass         + excluded.req_bypass,
    req_none           = req_none           + excluded.req_none,
    req_err            = req_err            + excluded.req_err,
    req_head           = req_head           + excluded.req_head,
    bytes_hit          = bytes_hit          + excluded.bytes_hit,
    bytes_miss         = bytes_miss         + excluded.bytes_miss,
    bytes_bypass       = bytes_bypass       + excluded.bytes_bypass,
    bytes_none         = bytes_none         + excluded.bytes_none,
    bytes_upstream     = bytes_upstream     + excluded.bytes_upstream,
    bytes_upstream_hit = bytes_upstream_hit + excluded.bytes_upstream_hit,
    time_sum_ms        = time_sum_ms        + excluded.time_sum_ms
"#,
    )?;
    for (k, c) in rows {
        st.execute(params![
            k.hour_ts,
            k.client_ip,
            k.repo,
            k.kind,
            c.req_hit,
            c.req_miss,
            c.req_bypass,
            c.req_none,
            c.req_err,
            c.req_head,
            c.bytes_hit,
            c.bytes_miss,
            c.bytes_bypass,
            c.bytes_none,
            c.bytes_upstream,
            c.bytes_upstream_hit,
            c.time_sum_ms,
        ])?;
    }
    Ok(())
}

fn write_paths(tx: &Transaction<'_>, rows: &[(PathKey, PathCounters)]) -> Result<()> {
    let mut st = tx.prepare_cached(
        r#"
INSERT INTO agg_path (day_ts, client_ip, repo, kind, path,
    reqs, req_hit, bytes, bytes_hit, bytes_upstream_hit, last_ts)
VALUES (?1,?2,?3,?4,?5, ?6,?7,?8,?9,?10,?11)
ON CONFLICT(day_ts, client_ip, repo, kind, path) DO UPDATE SET
    reqs               = reqs               + excluded.reqs,
    req_hit            = req_hit            + excluded.req_hit,
    bytes              = bytes              + excluded.bytes,
    bytes_hit          = bytes_hit          + excluded.bytes_hit,
    bytes_upstream_hit = bytes_upstream_hit + excluded.bytes_upstream_hit,
    last_ts            = max(last_ts, excluded.last_ts)
"#,
    )?;
    for (k, c) in rows {
        st.execute(params![
            k.day_ts,
            k.client_ip,
            k.repo,
            k.kind,
            k.path,
            c.reqs,
            c.req_hit,
            c.bytes,
            c.bytes_hit,
            c.bytes_upstream_hit,
            c.last_ts,
        ])?;
    }
    Ok(())
}

fn write_totals(tx: &Transaction<'_>, t: &Totals) -> Result<()> {
    let mut st = tx.prepare_cached(
        "INSERT INTO totals(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = value + excluded.value",
    )?;
    for (k, v) in [
        ("reqs", t.reqs),
        ("bytes_served", t.bytes_served),
        ("bytes_upstream", t.bytes_upstream),
        ("bytes_saved", t.bytes_saved),
        ("reqs_package", t.reqs_package),
        ("reqs_metadata", t.reqs_metadata),
        ("bytes_served_package", t.bytes_served_package),
        ("bytes_served_metadata", t.bytes_served_metadata),
        ("lines_ingested", t.lines_ingested),
        ("parse_errors", t.parse_errors),
        ("decode_failures", t.decode_failures),
    ] {
        if v != 0 {
            st.execute(params![k, v])?;
        }
    }
    Ok(())
}

/// A log file's ingest position, written in the same transaction as the
/// aggregates it authorises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub filename: String,
    pub inode: i64,
    pub offset: i64,
    pub size_seen: i64,
}

fn write_checkpoints(tx: &Transaction<'_>, rows: &[Checkpoint]) -> Result<()> {
    let mut st = tx.prepare_cached(
        "INSERT INTO ingest_state(filename, inode, offset, size_seen, updated_at)
         VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(filename) DO UPDATE SET
             inode = excluded.inode, offset = excluded.offset,
             size_seen = excluded.size_seen, updated_at = excluded.updated_at",
    )?;
    let now = crate::now_secs();
    for c in rows {
        st.execute(params![c.filename, c.inode, c.offset, c.size_seen, now])?;
    }
    Ok(())
}

/// Look up a checkpoint by filename.
pub fn checkpoint_by_name(conn: &Connection, filename: &str) -> Result<Option<Checkpoint>> {
    let mut st = conn.prepare_cached(
        "SELECT filename, inode, offset, size_seen FROM ingest_state WHERE filename = ?1",
    )?;
    let mut rows = st.query(params![filename])?;
    Ok(match rows.next()? {
        Some(r) => Some(Checkpoint {
            filename: r.get(0)?,
            inode: r.get(1)?,
            offset: r.get(2)?,
            size_seen: r.get(3)?,
        }),
        None => None,
    })
}

/// Look up a checkpoint by inode — used to adopt the offset of a RENAMED file.
pub fn checkpoint_by_inode(conn: &Connection, inode: i64) -> Result<Option<Checkpoint>> {
    let mut st = conn.prepare_cached(
        "SELECT filename, inode, offset, size_seen FROM ingest_state WHERE inode = ?1 LIMIT 1",
    )?;
    let mut rows = st.query(params![inode])?;
    Ok(match rows.next()? {
        Some(r) => Some(Checkpoint {
            filename: r.get(0)?,
            inode: r.get(1)?,
            offset: r.get(2)?,
            size_seen: r.get(3)?,
        }),
        None => None,
    })
}

/// Every tracked filename, for pruning `ingest_state` rows whose file is gone.
pub fn tracked_filenames(conn: &Connection) -> Result<Vec<String>> {
    let mut st = conn.prepare("SELECT filename FROM ingest_state")?;
    let out = st
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(out)
}

pub fn forget_file(conn: &Connection, filename: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM ingest_state WHERE filename = ?1",
        params![filename],
    )?;
    Ok(())
}

/// Read a lifetime counter.
pub fn total(conn: &Connection, key: &str) -> Result<i64> {
    let mut st = conn.prepare_cached("SELECT value FROM totals WHERE key = ?1")?;
    let mut rows = st.query(params![key])?;
    Ok(match rows.next()? {
        Some(r) => r.get(0)?,
        None => 0,
    })
}

/// Delete aggregates older than the retention window, then reclaim bounded
/// space. Never `VACUUM`: it needs 2x the database free and locks the world.
pub fn prune(conn: &mut Connection, hour_cutoff: i64, day_cutoff: i64) -> Result<(usize, usize)> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let h = tx.execute(
        "DELETE FROM agg_hour WHERE hour_ts < ?1",
        params![hour_cutoff],
    )?;
    let p = tx.execute(
        "DELETE FROM agg_path WHERE day_ts < ?1",
        params![day_cutoff],
    )?;
    tx.commit()?;
    // Bounded: ~800 KB per call, never a long stall.
    let _ = conn.execute("PRAGMA incremental_vacuum(200)", []);
    Ok((h, p))
}

/// Size of the database file plus its WAL sidecar, for the dashboard.
pub fn db_bytes(path: &std::path::Path) -> u64 {
    let mut n = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    for ext in ["-wal", "-shm"] {
        let mut p = path.as_os_str().to_os_string();
        p.push(ext);
        n += std::fs::metadata(std::path::PathBuf::from(p))
            .map(|m| m.len())
            .unwrap_or(0);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkgcache_ingest::{Batch, Drained, ParseError};

    fn mem_db() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "synchronous", "FULL").unwrap();
        migrate(&mut c).unwrap();
        c
    }

    fn ev(uri: &str, body: u64, upstream: &str, cache: &str) -> String {
        format!("1784418424.0\t192.168.10.10\tGET\t200\t{body}\t{upstream}\t{cache}\t0.1\t{uri}")
    }

    fn batch(lines: &[String]) -> Drained {
        let mut b = Batch::new();
        for l in lines {
            match pkgcache_ingest::parse_line_at(l, 1_784_418_424.0) {
                Ok(e) => b.add(&e),
                Err(ParseError::FieldCount(_)) | Err(_) => b.add_parse_error(),
            }
        }
        b.drain()
    }

    #[test]
    fn the_schema_is_strict_and_creates_cleanly() {
        let c = mem_db();
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND sql LIKE '%STRICT%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            n, 6,
            "meta, ingest_state, agg_hour, agg_path, totals, client_label"
        );
    }

    #[test]
    fn schema_version_is_recorded() {
        let c = mem_db();
        let v: String = c
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION.to_string());
    }

    #[test]
    fn repeated_upserts_accumulate_rather_than_replace() {
        // This is what makes a replay a re-count instead of a mis-count.
        let mut c = mem_db();
        let (h, p, t) = batch(&[ev("/debian/a/x_1_all.deb", 100, "-", "HIT")]);
        commit_batch(&mut c, &h, &p, &t, &[]).unwrap();
        commit_batch(&mut c, &h, &p, &t, &[]).unwrap();
        commit_batch(&mut c, &h, &p, &t, &[]).unwrap();

        let (hits, bytes): (i64, i64) = c
            .query_row("SELECT req_hit, bytes_hit FROM agg_hour", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(hits, 3);
        assert_eq!(bytes, 300);
        assert_eq!(total(&c, "bytes_saved").unwrap(), 300);
    }

    #[test]
    fn saved_is_derivable_from_the_stored_columns() {
        let mut c = mem_db();
        let (h, p, t) = batch(&[
            ev("/debian/a/x_1_all.deb", 1000, "-", "HIT"),
            ev("/debian/dists/trixie/InRelease", 800, "300", "REVALIDATED"),
            ev("/debian/b/y_1_all.deb", 5000, "5200", "MISS"),
        ]);
        commit_batch(&mut c, &h, &p, &t, &[]).unwrap();

        let derived: i64 = c
            .query_row(
                "SELECT sum(max(bytes_hit - bytes_upstream_hit, 0)) FROM agg_hour",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(derived, 1500, "1000 full HIT + 500 REVALIDATED net");
        assert_eq!(derived, total(&c, "bytes_saved").unwrap());
    }

    #[test]
    fn a_checkpoint_round_trips_by_name_and_by_inode() {
        let mut c = mem_db();
        let cp = Checkpoint {
            filename: "access-2026-07-18.log".into(),
            inode: 4242,
            offset: 1024,
            size_seen: 2048,
        };
        commit_batch(
            &mut c,
            &[],
            &[],
            &Totals::default(),
            std::slice::from_ref(&cp),
        )
        .unwrap();

        assert_eq!(checkpoint_by_name(&c, &cp.filename).unwrap().unwrap(), cp);
        // Rename adoption: the inode survives, the name does not.
        assert_eq!(checkpoint_by_inode(&c, 4242).unwrap().unwrap().offset, 1024);
        assert!(checkpoint_by_name(&c, "nope.log").unwrap().is_none());
    }

    #[test]
    fn a_checkpoint_update_replaces_rather_than_accumulating() {
        // Offsets are absolute positions, NOT deltas -- the one non-additive write.
        let mut c = mem_db();
        for off in [100, 250, 900] {
            let cp = Checkpoint {
                filename: "a.log".into(),
                inode: 1,
                offset: off,
                size_seen: off,
            };
            commit_batch(&mut c, &[], &[], &Totals::default(), &[cp]).unwrap();
        }
        assert_eq!(
            checkpoint_by_name(&c, "a.log").unwrap().unwrap().offset,
            900
        );
    }

    #[test]
    fn prune_drops_old_rows_and_keeps_recent_ones() {
        let mut c = mem_db();
        let (h, p, t) = batch(&[ev("/debian/a/x_1_all.deb", 100, "-", "HIT")]);
        commit_batch(&mut c, &h, &p, &t, &[]).unwrap();

        let (dh, dp) = prune(&mut c, 0, 0).unwrap();
        assert_eq!((dh, dp), (0, 0), "nothing older than epoch");

        let (dh, dp) = prune(&mut c, i64::MAX, i64::MAX).unwrap();
        assert_eq!((dh, dp), (1, 1));
        // Lifetime totals must survive pruning.
        assert_eq!(total(&c, "bytes_saved").unwrap(), 100);
    }

    #[test]
    fn tracked_filenames_and_forget_file_work() {
        let mut c = mem_db();
        for n in ["a.log", "b.log"] {
            let cp = Checkpoint {
                filename: n.into(),
                inode: 1,
                offset: 0,
                size_seen: 0,
            };
            commit_batch(&mut c, &[], &[], &Totals::default(), &[cp]).unwrap();
        }
        let mut names = tracked_filenames(&c).unwrap();
        names.sort();
        assert_eq!(names, vec!["a.log", "b.log"]);
        forget_file(&c, "a.log").unwrap();
        assert_eq!(tracked_filenames(&c).unwrap(), vec!["b.log"]);
    }

    #[test]
    fn an_unknown_total_reads_as_zero_rather_than_erroring() {
        let c = mem_db();
        assert_eq!(total(&c, "never_written").unwrap(), 0);
    }

    #[test]
    fn a_zero_delta_writes_no_total_row() {
        // Keeps the totals table free of noise rows on an idle tick.
        let mut c = mem_db();
        commit_batch(&mut c, &[], &[], &Totals::default(), &[]).unwrap();
        let n: i64 = c
            .query_row("SELECT count(*) FROM totals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}
