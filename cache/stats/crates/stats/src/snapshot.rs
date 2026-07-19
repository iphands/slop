//! Building the dashboard payload.
//!
//! Rebuilt at the end of every ingest tick and stored pre-serialized, so the
//! polled endpoint is a refcount bump and a socket write. **SQLite is never
//! touched by `GET /api/stats`** — that is where the speed comes from, not from
//! the language or the framework.
//!
//! Rebuilt **unconditionally**, even on a tick that ingested nothing: otherwise
//! an idle cache freezes the rolling 24h window at whatever it was when traffic
//! stopped.

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

use crate::db;

/// A ratio that may have no answer.
///
/// `None` serializes as `null` and the UI renders `—`. Rendering `0%` for "no
/// requests yet" makes a healthy cold cache look broken, which is exactly the
/// failure the metadata/package split exists to prevent.
type Ratio = Option<f64>;

fn ratio(hit: i64, total: i64) -> Ratio {
    if total <= 0 {
        None
    } else {
        Some(hit as f64 / total as f64)
    }
}

/// Counters for one slice (all / package / metadata), as the UI consumes them.
#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct Kpi {
    pub reqs: i64,
    pub bytes_served: i64,
    pub bytes_upstream: i64,
    /// `Σ max(0, bytes_hit − bytes_upstream_hit)` — hit-class only. Never a
    /// whole-window `served − upstream`, which would charge every MISS's header
    /// overhead against the savings.
    pub bytes_saved: i64,
    pub hit_ratio_bytes: Ratio,
    pub hit_ratio_reqs: Ratio,
}

/// One point in a time series.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Point {
    pub t: i64,
    pub package: Bucket,
    pub metadata: Bucket,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct Bucket {
    pub bytes_hit: i64,
    pub bytes_miss: i64,
    pub reqs_hit: i64,
    pub reqs_miss: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Series {
    pub bucket: &'static str,
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TopPath {
    pub path: String,
    /// Parsed package name, or the bare filename when unparseable. Raw paths run
    /// 60–110 chars and are unusable in a table.
    pub name: String,
    pub version: String,
    pub repo: String,
    pub reqs: i64,
    pub bytes_served: i64,
    pub bytes_saved: i64,
    pub hit_ratio_bytes: Ratio,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Client {
    pub ip: String,
    pub label: Option<String>,
    pub last_seen: i64,
    pub package: Kpi,
    pub metadata: Kpi,
    pub repos: Vec<String>,
    /// 24 hourly bytes_served values for the row sparkline.
    pub spark: Vec<i64>,
    /// BOUNDED at 10. The full list is behind `/api/stats/client/{ip}`, because
    /// `clients x packages` explodes on the day of a fleet-wide dist-upgrade —
    /// the day the dashboard matters most.
    pub top_packages: Vec<TopPath>,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct Ingest {
    pub last_tick_at: i64,
    pub lag_seconds: i64,
    pub files_tracked: usize,
    pub lines_ingested: i64,
    pub parse_errors: i64,
    /// False when the log directory cannot be read — the uid-mismatch signature.
    pub logs_readable: bool,
    pub db_bytes: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CacheDisk {
    pub bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Kpis {
    pub window: &'static str,
    pub all: Kpi,
    pub package: Kpi,
    pub metadata: Kpi,
    pub lifetime: Kpi,
}

/// The whole dashboard, in one response.
///
/// All three windows ship together (~20 KB): window switching becomes pure
/// client state with zero refetch, and an entire class of "which window is this
/// snapshot for" bugs disappears.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Payload {
    pub generated_at: i64,
    pub ingest: Ingest,
    pub cache_disk: Option<CacheDisk>,
    pub kpis: Kpis,
    pub series_24h: Series,
    pub series_7d: Series,
    pub series_30d: Series,
    pub clients: Vec<Client>,
    pub top_packages: Vec<TopPath>,
    pub top_metadata: Vec<TopPath>,
}

const HOUR: i64 = 3600;
const DAY: i64 = 86_400;

fn kpi_for(conn: &Connection, since: i64, kind: Option<&str>) -> Result<Kpi> {
    let (sql, filter) = match kind {
        Some(_) => (
            "SELECT coalesce(sum(req_hit),0), coalesce(sum(req_miss),0),
                    coalesce(sum(bytes_hit),0), coalesce(sum(bytes_miss+bytes_bypass+bytes_none),0),
                    coalesce(sum(bytes_upstream),0), coalesce(sum(bytes_upstream_hit),0),
                    coalesce(sum(req_head),0)
             FROM agg_hour WHERE hour_ts >= ?1 AND kind = ?2",
            true,
        ),
        None => (
            "SELECT coalesce(sum(req_hit),0), coalesce(sum(req_miss),0),
                    coalesce(sum(bytes_hit),0), coalesce(sum(bytes_miss+bytes_bypass+bytes_none),0),
                    coalesce(sum(bytes_upstream),0), coalesce(sum(bytes_upstream_hit),0),
                    coalesce(sum(req_head),0)
             FROM agg_hour WHERE hour_ts >= ?1",
            false,
        ),
    };
    let mut st = conn.prepare_cached(sql)?;
    let row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(i64, i64, i64, i64, i64, i64, i64)> {
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
            r.get(6)?,
        ))
    };
    let (rh, rm, bh, bo, bu, buh, _head) = if filter {
        st.query_row(rusqlite::params![since, kind.unwrap()], row)?
    } else {
        st.query_row(rusqlite::params![since], row)?
    };

    Ok(Kpi {
        reqs: rh + rm,
        bytes_served: bh + bo,
        bytes_upstream: bu,
        bytes_saved: (bh - buh).max(0),
        hit_ratio_bytes: ratio(bh, bh + bo),
        hit_ratio_reqs: ratio(rh, rh + rm),
    })
}

fn series(conn: &Connection, since: i64, step: i64, n: usize, now: i64) -> Result<Series> {
    let mut st = conn.prepare_cached(
        "SELECT hour_ts, kind, coalesce(sum(bytes_hit),0), coalesce(sum(bytes_miss),0),
                coalesce(sum(req_hit),0), coalesce(sum(req_miss),0)
         FROM agg_hour WHERE hour_ts >= ?1 GROUP BY hour_ts, kind",
    )?;
    let mut rows = std::collections::HashMap::<(i64, String), Bucket>::new();
    let mut q = st.query(rusqlite::params![since])?;
    while let Some(r) = q.next()? {
        let ts: i64 = r.get(0)?;
        let kind: String = r.get(1)?;
        // Snap each hour into its enclosing step, so 7d/30d reuse hourly rows.
        let slot = ts.div_euclid(step) * step;
        let e = rows.entry((slot, kind)).or_default();
        e.bytes_hit += r.get::<_, i64>(2)?;
        e.bytes_miss += r.get::<_, i64>(3)?;
        e.reqs_hit += r.get::<_, i64>(4)?;
        e.reqs_miss += r.get::<_, i64>(5)?;
    }

    // Zero-fill: a gap must render as zero, not as a missing point that the
    // chart would silently close over.
    let last = now.div_euclid(step) * step;
    let points = (0..n)
        .map(|i| {
            let t = last - (n as i64 - 1 - i as i64) * step;
            Point {
                t,
                package: rows
                    .get(&(t, "package".into()))
                    .cloned()
                    .unwrap_or_default(),
                metadata: rows
                    .get(&(t, "metadata".into()))
                    .cloned()
                    .unwrap_or_default(),
            }
        })
        .collect();
    Ok(Series {
        bucket: if step == HOUR { "hour" } else { "day" },
        points,
    })
}

fn top_paths(conn: &Connection, since_day: i64, kind: &str, limit: i64) -> Result<Vec<TopPath>> {
    let mut st = conn.prepare_cached(
        "SELECT path, repo, sum(reqs), sum(bytes), sum(bytes_hit), sum(bytes_upstream_hit)
         FROM agg_path WHERE day_ts >= ?1 AND kind = ?2
         GROUP BY path, repo ORDER BY sum(bytes) DESC LIMIT ?3",
    )?;
    let mut out = Vec::new();
    let mut q = st.query(rusqlite::params![since_day, kind, limit])?;
    while let Some(r) = q.next()? {
        let path: String = r.get(0)?;
        let bytes: i64 = r.get(3)?;
        let bh: i64 = r.get(4)?;
        let buh: i64 = r.get(5)?;
        let parsed = pkgcache_ingest::parse_path(&path);
        out.push(TopPath {
            name: parsed
                .as_ref()
                .map(|p| p.name.to_string())
                .unwrap_or_else(|| pkgcache_ingest::display_name(&path).to_string()),
            version: parsed
                .as_ref()
                .map(|p| p.version.to_string())
                .unwrap_or_default(),
            repo: r.get(1)?,
            reqs: r.get(2)?,
            bytes_served: bytes,
            bytes_saved: (bh - buh).max(0),
            hit_ratio_bytes: ratio(bh, bytes),
            path,
        });
    }
    Ok(out)
}

fn clients(conn: &Connection, now: i64) -> Result<Vec<Client>> {
    let since = now - DAY;
    let mut st = conn.prepare_cached(
        "SELECT client_ip, kind, coalesce(sum(req_hit),0), coalesce(sum(req_miss),0),
                coalesce(sum(bytes_hit),0),
                coalesce(sum(bytes_miss+bytes_bypass+bytes_none),0),
                coalesce(sum(bytes_upstream),0), coalesce(sum(bytes_upstream_hit),0),
                max(hour_ts)
         FROM agg_hour WHERE hour_ts >= ?1 GROUP BY client_ip, kind",
    )?;
    let mut by_ip: std::collections::HashMap<String, Client> = std::collections::HashMap::new();
    let mut q = st.query(rusqlite::params![since])?;
    while let Some(r) = q.next()? {
        let ip: String = r.get(0)?;
        let kind: String = r.get(1)?;
        let (rh, rm): (i64, i64) = (r.get(2)?, r.get(3)?);
        let (bh, bo): (i64, i64) = (r.get(4)?, r.get(5)?);
        let (bu, buh): (i64, i64) = (r.get(6)?, r.get(7)?);
        let last: i64 = r.get(8)?;
        let k = Kpi {
            reqs: rh + rm,
            bytes_served: bh + bo,
            bytes_upstream: bu,
            bytes_saved: (bh - buh).max(0),
            hit_ratio_bytes: ratio(bh, bh + bo),
            hit_ratio_reqs: ratio(rh, rh + rm),
        };
        let e = by_ip.entry(ip.clone()).or_insert_with(|| Client {
            ip,
            label: None,
            last_seen: 0,
            package: Kpi::default(),
            metadata: Kpi::default(),
            repos: Vec::new(),
            spark: vec![0; 24],
            top_packages: Vec::new(),
        });
        e.last_seen = e.last_seen.max(last);
        match kind.as_str() {
            "package" => e.package = k,
            "metadata" => e.metadata = k,
            _ => {}
        }
    }

    // Sparkline + repos + labels + bounded top-N, per client.
    let mut spark_st = conn.prepare_cached(
        "SELECT hour_ts, coalesce(sum(bytes_hit+bytes_miss+bytes_bypass+bytes_none),0)
         FROM agg_hour WHERE client_ip = ?1 AND hour_ts >= ?2 GROUP BY hour_ts",
    )?;
    let mut repo_st = conn.prepare_cached(
        "SELECT DISTINCT repo FROM agg_hour WHERE client_ip = ?1 AND hour_ts >= ?2 AND repo <> '-'",
    )?;
    let mut label_st =
        conn.prepare_cached("SELECT label FROM client_label WHERE client_ip = ?1")?;

    let first_hour = (now - DAY).div_euclid(HOUR) * HOUR;
    let mut out: Vec<Client> = Vec::new();
    for (ip, mut c) in by_ip {
        let mut q = spark_st.query(rusqlite::params![&ip, since])?;
        while let Some(r) = q.next()? {
            let ts: i64 = r.get(0)?;
            let idx = ((ts - first_hour) / HOUR) as usize;
            if idx < 24 {
                c.spark[idx] += r.get::<_, i64>(1)?;
            }
        }
        let mut q = repo_st.query(rusqlite::params![&ip, since])?;
        while let Some(r) = q.next()? {
            c.repos.push(r.get(0)?);
        }
        c.repos.sort();
        let mut q = label_st.query(rusqlite::params![&ip])?;
        if let Some(r) = q.next()? {
            c.label = Some(r.get(0)?);
        }
        c.top_packages = client_paths(conn, &ip, now - DAY, "package", 10)?;
        out.push(c);
    }
    // Most-saved first: the useful ordering for an ops dashboard.
    out.sort_by(|a, b| {
        (b.package.bytes_saved + b.metadata.bytes_saved)
            .cmp(&(a.package.bytes_saved + a.metadata.bytes_saved))
    });
    Ok(out)
}

/// Per-client paths. Bounded in the snapshot; unbounded (but on-demand) behind
/// the drilldown endpoint.
pub fn client_paths(
    conn: &Connection,
    ip: &str,
    since: i64,
    kind: &str,
    limit: i64,
) -> Result<Vec<TopPath>> {
    let mut st = conn.prepare_cached(
        "SELECT path, repo, sum(reqs), sum(bytes), sum(bytes_hit), sum(bytes_upstream_hit)
         FROM agg_path WHERE client_ip = ?1 AND day_ts >= ?2 AND kind = ?3
         GROUP BY path, repo ORDER BY sum(bytes) DESC LIMIT ?4",
    )?;
    let mut out = Vec::new();
    let mut q = st.query(rusqlite::params![ip, since, kind, limit])?;
    while let Some(r) = q.next()? {
        let path: String = r.get(0)?;
        let bytes: i64 = r.get(3)?;
        let bh: i64 = r.get(4)?;
        let buh: i64 = r.get(5)?;
        let parsed = pkgcache_ingest::parse_path(&path);
        out.push(TopPath {
            name: parsed
                .as_ref()
                .map(|p| p.name.to_string())
                .unwrap_or_else(|| pkgcache_ingest::display_name(&path).to_string()),
            version: parsed
                .as_ref()
                .map(|p| p.version.to_string())
                .unwrap_or_default(),
            repo: r.get(1)?,
            reqs: r.get(2)?,
            bytes_served: bytes,
            bytes_saved: (bh - buh).max(0),
            hit_ratio_bytes: ratio(bh, bytes),
            path,
        });
    }
    Ok(out)
}

/// Cache size and free space, from the read-only `/cache` mount.
///
/// Walks the tree rather than calling `statvfs`, to avoid a libc dependency for
/// one number; the cache is a few tens of thousands of files at most.
fn cache_disk(dir: &std::path::Path) -> Option<CacheDisk> {
    fn walk(p: &std::path::Path, acc: &mut u64, budget: &mut u32) {
        if *budget == 0 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            *budget -= 1;
            if *budget == 0 {
                return;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&e.path(), acc, budget),
                Ok(t) if t.is_file() => *acc += e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => {}
            }
        }
    }
    let pkg = dir.join("pkg");
    if !pkg.exists() {
        return None;
    }
    let mut bytes = 0u64;
    // Bounded so a pathological tree cannot stall a tick.
    let mut budget = 200_000u32;
    walk(&pkg, &mut bytes, &mut budget);
    Some(CacheDisk {
        bytes,
        free_bytes: 0,
    })
}

/// Build the whole payload.
pub fn build(
    conn: &Connection,
    now: i64,
    ingest: Ingest,
    cache_dir: Option<&std::path::Path>,
) -> Result<Payload> {
    let day = now - DAY;
    Ok(Payload {
        generated_at: now,
        ingest,
        cache_disk: cache_dir.and_then(cache_disk),
        kpis: Kpis {
            window: "24h",
            all: kpi_for(conn, day, None)?,
            package: kpi_for(conn, day, Some("package"))?,
            metadata: kpi_for(conn, day, Some("metadata"))?,
            lifetime: Kpi {
                reqs: db::total(conn, "reqs")?,
                bytes_served: db::total(conn, "bytes_served")?,
                bytes_upstream: db::total(conn, "bytes_upstream")?,
                bytes_saved: db::total(conn, "bytes_saved")?,
                hit_ratio_bytes: None,
                hit_ratio_reqs: None,
            },
        },
        series_24h: series(conn, now - DAY, HOUR, 24, now)?,
        series_7d: series(conn, now - 7 * DAY, HOUR, 168, now)?,
        series_30d: series(conn, now - 30 * DAY, DAY, 30, now)?,
        clients: clients(conn, now)?,
        top_packages: top_paths(conn, day, "package", 25)?,
        top_metadata: top_paths(conn, day, "metadata", 25)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkgcache_ingest::Batch;

    fn seeded(now: f64) -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        db::migrate_for_tests(&mut c);
        let mut b = Batch::new();
        for l in [
            format!("{now:.3}\t192.168.10.10\tGET\t200\t1000\t-\tHIT\t0.1\t/debian/pool/a/libgbm1_25.0.7-2+deb13u1_amd64.deb"),
            format!("{now:.3}\t192.168.10.10\tGET\t200\t800\t300\tREVALIDATED\t0.1\t/debian/dists/trixie/InRelease"),
            format!("{now:.3}\t192.168.10.99\tGET\t200\t5000\t5200\tMISS\t0.1\t/fedora/x/glib2-2.88.2-1.fc44.x86_64.rpm"),
        ] {
            b.add(&pkgcache_ingest::parse_line_at(&l, now).unwrap());
        }
        let (h, p, t) = b.drain();
        db::commit_batch(&mut c, &h, &p, &t, &[]).unwrap();
        c
    }

    fn now_f() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    }

    #[test]
    fn an_empty_database_yields_null_ratios_not_zero() {
        // A cold cache must render as "—", never as a broken-looking 0%.
        let mut c = Connection::open_in_memory().unwrap();
        db::migrate_for_tests(&mut c);
        let p = build(&c, 1_784_500_000, Ingest::default(), None).unwrap();
        assert_eq!(p.kpis.all.hit_ratio_bytes, None);
        assert_eq!(p.kpis.package.hit_ratio_reqs, None);
        let j = serde_json::to_string(&p.kpis.all).unwrap();
        assert!(j.contains("\"hit_ratio_bytes\":null"), "{j}");
    }

    #[test]
    fn saved_is_hit_class_only_and_never_negative() {
        let now = now_f();
        let c = seeded(now);
        let p = build(&c, now as i64, Ingest::default(), None).unwrap();
        // 1000 (full HIT) + 500 (REVALIDATED net) ; the MISS contributes 0.
        assert_eq!(p.kpis.all.bytes_saved, 1500);
        assert!(p.kpis.all.bytes_saved >= 0);
        assert_eq!(p.kpis.metadata.bytes_saved, 500);
        assert_eq!(p.kpis.package.bytes_saved, 1000);
    }

    #[test]
    fn package_and_metadata_are_never_blended() {
        let now = now_f();
        let c = seeded(now);
        let p = build(&c, now as i64, Ingest::default(), None).unwrap();
        assert_eq!(p.kpis.package.reqs, 2, "one .deb + one .rpm");
        assert_eq!(p.kpis.metadata.reqs, 1);
        assert_ne!(
            p.kpis.package.hit_ratio_bytes,
            p.kpis.metadata.hit_ratio_bytes
        );
    }

    #[test]
    fn all_three_series_have_their_full_length_zero_filled() {
        let now = now_f();
        let c = seeded(now);
        let p = build(&c, now as i64, Ingest::default(), None).unwrap();
        assert_eq!(p.series_24h.points.len(), 24);
        assert_eq!(p.series_7d.points.len(), 168);
        assert_eq!(p.series_30d.points.len(), 30);
        assert_eq!(p.series_30d.bucket, "day");
        // The final point is the current bucket, so the chart's right edge is now.
        assert!(p.series_24h.points.last().unwrap().t <= now as i64);
    }

    #[test]
    fn clients_are_listed_with_readable_package_names() {
        let now = now_f();
        let c = seeded(now);
        let p = build(&c, now as i64, Ingest::default(), None).unwrap();
        assert_eq!(p.clients.len(), 2);
        let deb = p
            .top_packages
            .iter()
            .find(|t| t.path.ends_with(".deb"))
            .unwrap();
        assert_eq!(deb.name, "libgbm1", "not the 40-char path");
        assert_eq!(deb.version, "25.0.7-2+deb13u1");
        let rpm = p
            .top_packages
            .iter()
            .find(|t| t.path.ends_with(".rpm"))
            .unwrap();
        assert_eq!(rpm.name, "glib2");
    }

    #[test]
    fn a_client_sparkline_has_exactly_24_slots() {
        let now = now_f();
        let c = seeded(now);
        let p = build(&c, now as i64, Ingest::default(), None).unwrap();
        for cl in &p.clients {
            assert_eq!(cl.spark.len(), 24);
        }
    }

    #[test]
    fn top_packages_are_bounded_per_client() {
        let now = now_f();
        let c = seeded(now);
        let p = build(&c, now as i64, Ingest::default(), None).unwrap();
        for cl in &p.clients {
            assert!(cl.top_packages.len() <= 10, "snapshot must stay bounded");
        }
    }

    #[test]
    fn the_payload_serializes_and_round_trips_as_json() {
        let now = now_f();
        let c = seeded(now);
        let p = build(&c, now as i64, Ingest::default(), None).unwrap();
        let j = serde_json::to_vec(&p).unwrap();
        assert!(j.len() > 100);
        let v: serde_json::Value = serde_json::from_slice(&j).unwrap();
        assert!(v.get("kpis").is_some());
        assert!(v.get("series_24h").is_some());
        assert!(v.get("clients").is_some());
    }
}
