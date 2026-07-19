//! Log-file pruning and database retention.
//!
//! # The hazard this module exists to avoid
//!
//! nginx's `open_log_file_cache … valid=1m` means a worker keeps a log file's
//! descriptor for up to a minute after its last write. **Unlink a file nginx
//! still holds and it keeps appending to an unreachable inode** — every request
//! in that window vanishes with no error anywhere.
//!
//! So a log is deleted only when ALL of:
//!
//! 1. its date is older than `log_retention_days`, and
//! 2. it is fully ingested (`offset >= size`), and
//! 3. it is **not today's or yesterday's**, unconditionally.
//!
//! Condition 3 is the fd-cache margin, with about 1,440x the slack actually
//! required. Condition 2 is why this cannot be a host-side `find -mtime +N
//! -delete`: only this process knows what it has consumed.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::db;

/// `access-nodate.log` is never pruned: there is no date to compare, and a
/// non-empty one means the `$time_iso8601` map failed, which is a structural
/// problem worth keeping the evidence for.
pub const NODATE_LOG: &str = "access-nodate.log";

/// Civil date from a unix timestamp (UTC). Containers run `TZ=UTC`, and nginx
/// names files by local date, so the two agree by construction.
///
/// Howard Hinnant's `civil_from_days`, so no date dependency is needed for what
/// amounts to one calculation.
pub fn ymd_from_epoch(secs: i64) -> (i64, u32, u32) {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `access-YYYY-MM-DD.log` for a timestamp.
pub fn log_name_for(secs: i64) -> String {
    let (y, m, d) = ymd_from_epoch(secs);
    format!("access-{y:04}-{m:02}-{d:02}.log")
}

/// Extract the `YYYY-MM-DD` from a log filename, if it has one.
fn date_of(filename: &str) -> Option<&str> {
    filename
        .strip_prefix("access-")
        .and_then(|r| r.strip_suffix(".log"))
        .filter(|s| s.len() == 10)
}

/// Decide whether one log file may be deleted. Pure, so every rule is testable
/// without touching a filesystem.
///
/// `today` / `yesterday` are `YYYY-MM-DD` strings; `cutoff` is the oldest date
/// that may be kept.
pub fn may_delete(
    filename: &str,
    cutoff: &str,
    today: &str,
    yesterday: &str,
    fully_ingested: bool,
) -> Result<bool, &'static str> {
    if filename == NODATE_LOG {
        return Err("access-nodate.log is never pruned");
    }
    let Some(date) = date_of(filename) else {
        return Err("not a dated log file");
    };
    if date == today || date == yesterday {
        // The open_log_file_cache margin. Non-negotiable.
        return Ok(false);
    }
    if date >= cutoff {
        return Ok(false);
    }
    if !fully_ingested {
        return Err("older than retention but NOT fully ingested");
    }
    Ok(true)
}

/// Result of one prune pass, for logging.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub logs_deleted: usize,
    pub logs_kept_unread: usize,
    pub state_rows_dropped: usize,
    pub hour_rows_deleted: usize,
    pub path_rows_deleted: usize,
    pub nodate_bytes: u64,
}

/// Run retention: delete consumed old logs, drop orphan checkpoints, and trim
/// the aggregate tables. Called hourly, never per tick.
pub fn run(
    conn: &mut Connection,
    logs_dir: &Path,
    now: i64,
    log_retention_days: i64,
    db_retention_days: i64,
) -> Result<PruneReport> {
    let mut rep = PruneReport::default();

    let today = log_name_for(now);
    let yesterday = log_name_for(now - 86_400);
    let cutoff = log_name_for(now - log_retention_days * 86_400);
    let (today, yesterday, cutoff) = (
        date_of(&today).unwrap_or(""),
        date_of(&yesterday).unwrap_or(""),
        date_of(&cutoff).unwrap_or(""),
    );

    let (files, readable) = crate::tail::discover(logs_dir)?;
    if !readable {
        // Nothing to do, and discover() has already logged loudly.
        return Ok(rep);
    }

    for path in &files {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if name == NODATE_LOG {
            rep.nodate_bytes = meta.len();
            if meta.len() > 0 {
                tracing::warn!(
                    bytes = meta.len(),
                    "access-nodate.log is non-empty: the $time_iso8601 map failed, \
                     so log rotation is not happening. Never pruned."
                );
            }
            continue;
        }

        let cp = db::checkpoint_by_name(conn, name)?;
        let fully = cp.as_ref().is_some_and(|c| c.offset >= meta.len() as i64);

        match may_delete(name, cutoff, today, yesterday, fully) {
            Ok(true) => match std::fs::remove_file(path) {
                Ok(()) => {
                    tracing::info!(file = %name, "pruned fully-ingested log");
                    db::forget_file(conn, name)?;
                    rep.logs_deleted += 1;
                }
                Err(e) => tracing::warn!(file = %name, "could not prune: {e}"),
            },
            Ok(false) => {}
            Err(why) => {
                if why.starts_with("older than retention") {
                    // Keep unread data and retry next hour rather than lose it.
                    tracing::warn!(file = %name, "{why}; keeping it");
                    rep.logs_kept_unread += 1;
                }
            }
        }
    }

    // Drop checkpoints for files that are gone, so ingest_state cannot grow
    // without bound.
    let on_disk: std::collections::HashSet<String> = files
        .iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
        .collect();
    for tracked in db::tracked_filenames(conn)? {
        if !on_disk.contains(&tracked) {
            db::forget_file(conn, &tracked)?;
            rep.state_rows_dropped += 1;
        }
    }

    let (h, p) = db::prune(
        conn,
        now - db_retention_days * 86_400,
        now - db_retention_days * 86_400,
    )?;
    rep.hour_rows_deleted = h;
    rep.path_rows_deleted = p;
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-07-19";
    const YESTERDAY: &str = "2026-07-18";
    const CUTOFF: &str = "2026-07-16"; // 3-day retention

    #[test]
    fn epoch_to_civil_date_is_correct() {
        // 1784418424 = 2026-07-18 (the session's real log date).
        assert_eq!(ymd_from_epoch(1_784_418_424), (2026, 7, 18));
        assert_eq!(ymd_from_epoch(0), (1970, 1, 1));
        // A leap day, since the algorithm's whole job is calendar edge cases.
        assert_eq!(ymd_from_epoch(1_709_164_800), (2024, 2, 29));
        assert_eq!(log_name_for(1_784_418_424), "access-2026-07-18.log");
    }

    #[test]
    fn todays_log_is_never_deleted() {
        assert_eq!(
            may_delete("access-2026-07-19.log", CUTOFF, TODAY, YESTERDAY, true),
            Ok(false)
        );
    }

    #[test]
    fn yesterdays_log_is_never_deleted_even_when_old_enough() {
        // The open_log_file_cache margin: nginx may still hold the fd.
        assert_eq!(
            may_delete(
                "access-2026-07-18.log",
                "2026-07-19",
                TODAY,
                YESTERDAY,
                true
            ),
            Ok(false)
        );
    }

    #[test]
    fn an_old_fully_ingested_log_is_deletable() {
        assert_eq!(
            may_delete("access-2026-07-10.log", CUTOFF, TODAY, YESTERDAY, true),
            Ok(true)
        );
    }

    #[test]
    fn an_old_but_unread_log_is_kept_not_deleted() {
        // Losing un-ingested data is worse than keeping a file another hour.
        assert!(may_delete("access-2026-07-10.log", CUTOFF, TODAY, YESTERDAY, false).is_err());
    }

    #[test]
    fn a_log_inside_the_retention_window_is_kept() {
        assert_eq!(
            may_delete("access-2026-07-17.log", CUTOFF, TODAY, YESTERDAY, true),
            Ok(false)
        );
    }

    #[test]
    fn the_nodate_log_is_never_pruned() {
        assert!(may_delete(NODATE_LOG, CUTOFF, TODAY, YESTERDAY, true).is_err());
    }

    #[test]
    fn a_non_log_filename_is_refused() {
        assert!(may_delete("error.log", CUTOFF, TODAY, YESTERDAY, true).is_err());
    }

    #[test]
    fn prune_deletes_only_what_it_should() {
        use std::io::Write;
        let d = tempfile::tempdir().unwrap();
        let now = 1_784_500_000i64; // 2026-07-19
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate_for_tests(&mut conn);

        // old + fully ingested -> deleted; old + unread -> kept; today -> kept.
        for (name, ingested) in [
            ("access-2026-07-01.log", true),
            ("access-2026-07-02.log", false),
            (&log_name_for(now), true),
            (NODATE_LOG, true),
        ] {
            let p = d.path().join(name);
            let mut f = std::fs::File::create(&p).unwrap();
            f.write_all(b"x\n").unwrap();
            if ingested {
                let cp = crate::db::Checkpoint {
                    filename: name.to_string(),
                    inode: 1,
                    offset: 2,
                    size_seen: 2,
                };
                crate::db::commit_batch(
                    &mut conn,
                    &[],
                    &[],
                    &pkgcache_ingest::Totals::default(),
                    &[cp],
                )
                .unwrap();
            }
        }

        let rep = run(&mut conn, d.path(), now, 3, 30).unwrap();
        assert_eq!(rep.logs_deleted, 1, "only the old, fully-ingested one");
        assert_eq!(rep.logs_kept_unread, 1);
        assert!(!d.path().join("access-2026-07-01.log").exists());
        assert!(
            d.path().join("access-2026-07-02.log").exists(),
            "unread: kept"
        );
        assert!(d.path().join(log_name_for(now)).exists(), "today: kept");
        assert!(d.path().join(NODATE_LOG).exists(), "nodate: never pruned");
        assert!(rep.nodate_bytes > 0);
    }

    #[test]
    fn orphan_checkpoints_are_dropped() {
        let d = tempfile::tempdir().unwrap();
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate_for_tests(&mut conn);
        let cp = crate::db::Checkpoint {
            filename: "access-2026-01-01.log".into(),
            inode: 9,
            offset: 5,
            size_seen: 5,
        };
        crate::db::commit_batch(
            &mut conn,
            &[],
            &[],
            &pkgcache_ingest::Totals::default(),
            &[cp],
        )
        .unwrap();
        let rep = run(&mut conn, d.path(), 1_784_500_000, 3, 30).unwrap();
        assert_eq!(rep.state_rows_dropped, 1);
        assert!(crate::db::tracked_filenames(&conn).unwrap().is_empty());
    }
}
