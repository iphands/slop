//! Reading nginx's dated access logs, exactly once.
//!
//! One tick: discover files, resume each from its checkpoint, read a bounded
//! chunk, parse whole lines only, and commit the aggregates **and** the advanced
//! offsets in one transaction.
//!
//! Ordering is a tidiness choice, not a correctness one — every line carries its
//! own `$msec`, so buckets are right regardless of read order. That removes the
//! temptation to build ordering machinery.

use std::fs::File;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pkgcache_ingest::{parse_line, split_complete_lines, Batch};
use rusqlite::Connection;

use crate::db::{self, Checkpoint};

/// Largest chunk read from one file per tick. Bounds RSS: a week of backlog
/// drains at this rate per tick without ever spiking memory.
pub const READ_CAP: usize = 16 * 1024 * 1024;

/// Outcome of one tick, for logging and the health payload.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickReport {
    pub files_seen: usize,
    pub files_advanced: usize,
    pub bytes_read: u64,
    pub lines: i64,
    pub parse_errors: i64,
    /// False when `/logs` exists but cannot be read — the signature of a uid
    /// mismatch between the two containers, whose symptom is otherwise a
    /// dashboard of silent zeros with no error anywhere.
    pub logs_readable: bool,
}

/// Is this one of nginx's stats logs?
///
/// `access-YYYY-MM-DD.log`, plus `access-nodate.log` for the case where the
/// `$time_iso8601` map failed. ISO dates sort lexicographically = chronologically
/// and `nodate` sorts last, which is where we want it.
pub fn is_log_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("access-") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".log") else {
        return false;
    };
    stem == "nodate" || is_iso_date(stem)
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7) || c.is_ascii_digit())
}

/// List the log files to process, oldest name first.
pub fn discover(dir: &Path) -> Result<(Vec<PathBuf>, bool)> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // THE high-value error message in this whole service.
            tracing::error!(
                dir = %dir.display(),
                "PERMISSION DENIED reading the log directory. nginx and the stats \
                 service must run as the SAME uid:gid and be launched the same way \
                 (same --userns flags). Until this is fixed the dashboard will show \
                 zeros with no other symptom."
            );
            return Ok((Vec::new(), false));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(dir = %dir.display(), "log directory does not exist yet");
            return Ok((Vec::new(), false));
        }
        Err(e) => return Err(e).with_context(|| format!("read_dir {}", dir.display())),
    };

    let mut out: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_log_name)
        })
        .collect();
    out.sort();
    Ok((out, true))
}

/// Where to resume reading a file, given its checkpoint.
#[derive(Debug, PartialEq, Eq)]
enum Resume {
    /// Continue from this byte offset.
    At(i64),
    /// Start over — the file is new, replaced, or was truncated.
    FromStart(&'static str),
}

fn decide_offset(conn: &Connection, filename: &str, inode: i64, size: i64) -> Result<Resume> {
    if let Some(cp) = db::checkpoint_by_name(conn, filename)? {
        if cp.inode != inode {
            return Ok(Resume::FromStart("log file replaced (inode changed)"));
        }
        if size < cp.offset {
            return Ok(Resume::FromStart("log file truncated (size < offset)"));
        }
        return Ok(Resume::At(cp.offset));
    }
    // No row for this NAME -- but the same inode under a different name means the
    // file was renamed. Adopting its offset is what stops a logrotate from
    // re-ingesting the whole file and doubling every number.
    if let Some(cp) = db::checkpoint_by_inode(conn, inode)? {
        if cp.offset <= size {
            tracing::info!(
                from = %cp.filename, to = %filename,
                "adopting checkpoint of a renamed log file"
            );
            return Ok(Resume::At(cp.offset));
        }
    }
    Ok(Resume::At(0))
}

/// Run one ingest tick over `dir`, committing into `conn`.
pub fn tick(conn: &mut Connection, dir: &Path) -> Result<TickReport> {
    let (files, logs_readable) = discover(dir)?;
    let mut report = TickReport {
        files_seen: files.len(),
        logs_readable,
        ..Default::default()
    };

    let mut batch = Batch::new();
    let mut checkpoints: Vec<Checkpoint> = Vec::new();

    for path in &files {
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Pruned between readdir and open: skip silently, it is not an error.
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                tracing::warn!(file = %filename, "cannot open: {e}");
                continue;
            }
        };
        let meta = file.metadata()?;
        let inode = meta.ino() as i64;
        let size = meta.len() as i64;

        let start = match decide_offset(conn, &filename, inode, size)? {
            Resume::At(o) => o,
            Resume::FromStart(why) => {
                tracing::warn!(file = %filename, "{why}; restarting from 0");
                0
            }
        };
        if size <= start {
            continue; // nothing new
        }

        let want = ((size - start) as usize).min(READ_CAP);
        let mut buf = vec![0u8; want];
        let got = file
            .read_at(&mut buf, start as u64)
            .with_context(|| format!("read {filename} at {start}"))?;
        buf.truncate(got);
        report.bytes_read += got as u64;

        let (lines, mut consumed) = split_complete_lines(&buf);
        if consumed == 0 {
            if got >= READ_CAP {
                // A single line longer than the read cap: corrupt or truncated.
                // Skip the whole chunk rather than stalling this file -- and
                // every file behind it -- forever.
                tracing::error!(
                    file = %filename, offset = start,
                    "no newline within the {READ_CAP}-byte read cap; skipping the chunk"
                );
                batch.add_parse_error();
                consumed = got;
            } else {
                continue; // a partial line still being written; retry next tick
            }
        }

        for line in std::str::from_utf8(lines)
            .unwrap_or_default()
            .split_inclusive('\n')
        {
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                continue;
            }
            match parse_line(line) {
                Ok(ev) => batch.add(&ev),
                Err(_) => batch.add_parse_error(),
            }
        }

        checkpoints.push(Checkpoint {
            filename,
            inode,
            offset: start + consumed as i64,
            size_seen: size,
        });
        report.files_advanced += 1;
    }

    report.lines = batch.totals().lines_ingested;
    report.parse_errors = batch.totals().parse_errors;

    let (hours, paths, totals) = batch.drain();
    // THE invariant: aggregates and offsets land together, or neither does.
    db::commit_batch(conn, &hours, &paths, &totals, &checkpoints)?;
    Ok(report)
}

/// Hold the single-writer lock for the process lifetime.
///
/// Two writers would each read `offset=N`, each parse the same bytes, and each
/// apply `+delta` — a silent 2x on every number. SQLite's own locking does not
/// prevent this: it serialises the writes, it does not deduplicate the intent.
pub struct WriterLock {
    /// The guard releases the flock when dropped, so the lock lives exactly as
    /// long as this value.
    _guard: fd_lock::RwLockWriteGuard<'static, File>,
}

/// Take the exclusive writer lock, or fail.
///
/// The `RwLock` is intentionally leaked: the guard borrows it, so the two cannot
/// live in one struct without self-reference. One small allocation held for the
/// process lifetime is the honest trade — this is acquired exactly once at
/// startup and released by process exit.
pub fn acquire_writer_lock(path: &Path) -> Result<WriterLock> {
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .with_context(|| format!("open lock file {}", path.display()))?;
    let lock: &'static mut fd_lock::RwLock<File> = Box::leak(Box::new(fd_lock::RwLock::new(file)));
    // try_write() is non-blocking: fail fast rather than queue behind a twin.
    match lock.try_write() {
        Ok(guard) => Ok(WriterLock { _guard: guard }),
        Err(e) => anyhow::bail!(
            "another pkgcache-stats already holds {} ({e}). Two writers would \
             double every number; refusing to start.",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn db_conn() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::db::migrate_for_tests(&mut c);
        c
    }

    fn now() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    }

    fn line(uri: &str, body: u64, cache: &str) -> String {
        format!(
            "{:.3}\t192.168.10.10\tGET\t200\t{body}\t-\t{cache}\t0.1\t{uri}\n",
            now()
        )
    }

    fn write_log(dir: &Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        let mut f = File::create(&p).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        p
    }

    #[test]
    fn only_nginx_stats_logs_are_discovered() {
        assert!(is_log_name("access-2026-07-18.log"));
        assert!(is_log_name("access-nodate.log"));
        assert!(!is_log_name("access-2026-7-8.log"), "must be zero-padded");
        assert!(!is_log_name("access-.log"));
        assert!(!is_log_name("error.log"));
        assert!(!is_log_name("access-2026-07-18.log.gz"));
    }

    #[test]
    fn discovery_sorts_chronologically_with_nodate_last() {
        let d = tempfile::tempdir().unwrap();
        for n in [
            "access-2026-07-19.log",
            "access-nodate.log",
            "access-2026-07-17.log",
        ] {
            write_log(d.path(), n, "");
        }
        let (files, ok) = discover(d.path()).unwrap();
        assert!(ok);
        let names: Vec<_> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "access-2026-07-17.log",
                "access-2026-07-19.log",
                "access-nodate.log"
            ]
        );
    }

    #[test]
    fn a_missing_log_directory_reports_unreadable_rather_than_erroring() {
        let (files, ok) = discover(Path::new("/nonexistent/pkgcache/logs")).unwrap();
        assert!(files.is_empty());
        assert!(!ok, "logs_readable must be false so the UI can say so");
    }

    #[test]
    fn a_tick_ingests_and_checkpoints_in_one_pass() {
        let d = tempfile::tempdir().unwrap();
        let content = format!(
            "{}{}",
            line("/debian/pool/a/x_1_all.deb", 100, "HIT"),
            line("/debian/dists/trixie/InRelease", 200, "MISS")
        );
        write_log(d.path(), "access-2026-07-18.log", &content);

        let mut c = db_conn();
        let r = tick(&mut c, d.path()).unwrap();
        assert_eq!(r.lines, 2);
        assert_eq!(r.files_advanced, 1);
        assert!(r.logs_readable);

        let cp = db::checkpoint_by_name(&c, "access-2026-07-18.log")
            .unwrap()
            .unwrap();
        assert_eq!(cp.offset as usize, content.len());
    }

    #[test]
    fn re_running_a_tick_over_unchanged_files_is_a_no_op() {
        // Idempotency: this is what makes a crash-and-replay safe.
        let d = tempfile::tempdir().unwrap();
        write_log(
            d.path(),
            "access-2026-07-18.log",
            &line("/debian/pool/a/x_1_all.deb", 100, "HIT"),
        );
        let mut c = db_conn();
        tick(&mut c, d.path()).unwrap();
        let first = db::total(&c, "bytes_saved").unwrap();

        for _ in 0..3 {
            let r = tick(&mut c, d.path()).unwrap();
            assert_eq!(r.lines, 0, "nothing new to read");
        }
        assert_eq!(db::total(&c, "bytes_saved").unwrap(), first);
    }

    #[test]
    fn appended_lines_are_picked_up_from_the_checkpoint() {
        let d = tempfile::tempdir().unwrap();
        let p = write_log(
            d.path(),
            "access-2026-07-18.log",
            &line("/debian/pool/a/x_1_all.deb", 100, "HIT"),
        );
        let mut c = db_conn();
        tick(&mut c, d.path()).unwrap();

        let mut f = File::options().append(true).open(&p).unwrap();
        f.write_all(line("/debian/pool/a/y_1_all.deb", 50, "HIT").as_bytes())
            .unwrap();
        let r = tick(&mut c, d.path()).unwrap();
        assert_eq!(r.lines, 1, "only the new line");
        assert_eq!(db::total(&c, "bytes_saved").unwrap(), 150);
    }

    #[test]
    fn a_partial_trailing_line_is_not_consumed_until_it_is_complete() {
        let d = tempfile::tempdir().unwrap();
        let full = line("/debian/pool/a/x_1_all.deb", 100, "HIT");
        let partial = "1784418424.0\t192.168.10.10\tGET\t200\t50\t-\tHIT\t0.1\t/deb";
        let p = write_log(
            d.path(),
            "access-2026-07-18.log",
            &format!("{full}{partial}"),
        );

        let mut c = db_conn();
        let r = tick(&mut c, d.path()).unwrap();
        assert_eq!(r.lines, 1, "the partial line must not be parsed");
        let cp = db::checkpoint_by_name(&c, "access-2026-07-18.log")
            .unwrap()
            .unwrap();
        assert_eq!(
            cp.offset as usize,
            full.len(),
            "offset stops at the newline"
        );

        // Completing the line makes it ingestable, with no double count.
        let mut f = File::options().append(true).open(&p).unwrap();
        f.write_all(b"ian/pool/a/z_1_all.deb\n").unwrap();
        let r = tick(&mut c, d.path()).unwrap();
        assert_eq!(r.lines, 1);
    }

    #[test]
    fn a_truncated_file_restarts_from_zero() {
        let d = tempfile::tempdir().unwrap();
        let p = write_log(
            d.path(),
            "access-2026-07-18.log",
            &line("/debian/pool/a/x_1_all.deb", 100, "HIT"),
        );
        let mut c = db_conn();
        tick(&mut c, d.path()).unwrap();

        // Truncate and write one shorter line.
        File::create(&p)
            .unwrap()
            .write_all(line("/debian/pool/a/y_1_all.deb", 7, "HIT").as_bytes())
            .unwrap();
        let r = tick(&mut c, d.path()).unwrap();
        assert_eq!(r.lines, 1, "detected truncation and re-read from 0");
    }

    #[test]
    fn a_replaced_file_with_the_same_name_restarts_from_zero() {
        let d = tempfile::tempdir().unwrap();
        let name = "access-2026-07-18.log";
        write_log(
            d.path(),
            name,
            &line("/debian/pool/a/x_1_all.deb", 100, "HIT"),
        );
        let mut c = db_conn();
        tick(&mut c, d.path()).unwrap();

        // Remove and recreate: same name, new inode, same length.
        std::fs::remove_file(d.path().join(name)).unwrap();
        write_log(
            d.path(),
            name,
            &line("/debian/pool/a/x_1_all.deb", 100, "HIT"),
        );
        let r = tick(&mut c, d.path()).unwrap();
        assert_eq!(r.lines, 1, "inode change must force a re-read");
    }

    #[test]
    fn a_renamed_file_adopts_its_offset_instead_of_re_ingesting() {
        // The logrotate case: without inode adoption every number doubles.
        let d = tempfile::tempdir().unwrap();
        let old = write_log(
            d.path(),
            "access-2026-07-18.log",
            &line("/debian/pool/a/x_1_all.deb", 100, "HIT"),
        );
        let mut c = db_conn();
        tick(&mut c, d.path()).unwrap();
        let before = db::total(&c, "bytes_saved").unwrap();

        std::fs::rename(&old, d.path().join("access-2026-07-19.log")).unwrap();
        let r = tick(&mut c, d.path()).unwrap();
        assert_eq!(r.lines, 0, "same inode, same offset: nothing to re-read");
        assert_eq!(db::total(&c, "bytes_saved").unwrap(), before);
    }

    #[test]
    fn a_malformed_line_is_counted_and_skipped_without_stalling() {
        let d = tempfile::tempdir().unwrap();
        let content = format!(
            "{}garbage line\n{}",
            line("/debian/pool/a/x_1_all.deb", 100, "HIT"),
            line("/debian/pool/a/y_1_all.deb", 50, "HIT")
        );
        write_log(d.path(), "access-2026-07-18.log", &content);
        let mut c = db_conn();
        let r = tick(&mut c, d.path()).unwrap();
        assert_eq!(r.lines, 2);
        assert_eq!(r.parse_errors, 1);
        let cp = db::checkpoint_by_name(&c, "access-2026-07-18.log")
            .unwrap()
            .unwrap();
        assert_eq!(
            cp.offset as usize,
            content.len(),
            "bad bytes still consumed"
        );
    }

    #[test]
    fn the_writer_lock_refuses_a_second_holder() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".ingest.lock");
        let _first = acquire_writer_lock(&p).unwrap();
        assert!(
            acquire_writer_lock(&p).is_err(),
            "a second writer would double every number"
        );
    }
}
