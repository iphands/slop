//! In-memory aggregation of a tick's worth of events.
//!
//! The reader accumulates every line of a tick into one [`Batch`], then drains
//! it into rows that are UPSERTed additively inside a single transaction with
//! the checkpoint offset. Additivity is what makes a re-ingest a re-count rather
//! than a mis-count: replaying the same bytes produces the same deltas.

use std::collections::HashMap;

use crate::classify::{classify, Kind};
use crate::line::{CacheClass, Event};

/// Bucket a timestamp into its UTC hour.
pub fn hour_ts(msec: f64) -> i64 {
    (msec as i64).div_euclid(3600) * 3600
}

/// Bucket a timestamp into its UTC day.
pub fn day_ts(msec: f64) -> i64 {
    (msec as i64).div_euclid(86_400) * 86_400
}

/// Cap on distinct paths per (day, client) before folding into [`OTHER_PATH`].
///
/// A 404 storm from a LAN scanner or a broken script is the realistic blowup
/// shape. Package traffic never approaches this: the largest observed real run
/// was 32 distinct paths.
pub const MAX_PATHS_PER_CLIENT_DAY: usize = 5_000;

/// Placeholder path used once a client-day exceeds [`MAX_PATHS_PER_CLIENT_DAY`].
pub const OTHER_PATH: &str = "(other)";

/// Key for the hourly fact table.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HourKey {
    pub hour_ts: i64,
    pub client_ip: String,
    pub repo: String,
    pub kind: &'static str,
}

/// Key for the daily path table.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathKey {
    pub day_ts: i64,
    pub client_ip: String,
    pub repo: String,
    pub kind: &'static str,
    pub path: String,
}

/// Counters for one hourly bucket. Every field is additive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HourCounters {
    pub req_hit: i64,
    pub req_miss: i64,
    pub req_bypass: i64,
    pub req_none: i64,
    pub req_err: i64,
    pub req_head: i64,

    pub bytes_hit: i64,
    pub bytes_miss: i64,
    pub bytes_bypass: i64,
    pub bytes_none: i64,

    /// Bytes read from upstream across ALL classes.
    pub bytes_upstream: i64,
    /// Bytes read from upstream on HIT-class responses only.
    ///
    /// Stored separately because `bytes_saved` is a hit-class-only subtraction:
    /// `Σ(bytes_hit) − Σ(bytes_upstream_hit)`. With a single combined column the
    /// formula could not be reconstructed at query time, and a whole-window
    /// `served − upstream` would charge every MISS's header overhead against the
    /// savings. See `context/distilled.md`.
    pub bytes_upstream_hit: i64,

    pub time_sum_ms: i64,
}

/// Counters for one (day, client, path) row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathCounters {
    pub reqs: i64,
    pub req_hit: i64,
    pub bytes: i64,
    pub bytes_hit: i64,
    pub bytes_upstream_hit: i64,
    pub last_ts: i64,
}

/// Lifetime counters, never pruned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Totals {
    pub reqs: i64,
    pub bytes_served: i64,
    pub bytes_upstream: i64,
    pub bytes_saved: i64,
    pub reqs_package: i64,
    pub reqs_metadata: i64,
    pub bytes_served_package: i64,
    pub bytes_served_metadata: i64,
    pub lines_ingested: i64,
    pub parse_errors: i64,
    pub decode_failures: i64,
}

impl Totals {
    /// Fold another delta into this one. Used to merge per-file batches.
    pub fn add(&mut self, o: &Totals) {
        self.reqs += o.reqs;
        self.bytes_served += o.bytes_served;
        self.bytes_upstream += o.bytes_upstream;
        self.bytes_saved += o.bytes_saved;
        self.reqs_package += o.reqs_package;
        self.reqs_metadata += o.reqs_metadata;
        self.bytes_served_package += o.bytes_served_package;
        self.bytes_served_metadata += o.bytes_served_metadata;
        self.lines_ingested += o.lines_ingested;
        self.parse_errors += o.parse_errors;
        self.decode_failures += o.decode_failures;
    }
}

/// What [`Batch::drain`] yields: hourly rows, path rows, and the lifetime delta.
///
/// A named alias rather than a bare tuple, so callers (and their test helpers)
/// can spell the type without tripping `clippy::type_complexity`.
pub type Drained = (
    Vec<(HourKey, HourCounters)>,
    Vec<(PathKey, PathCounters)>,
    Totals,
);

/// A tick's accumulated deltas.
#[derive(Debug, Default)]
pub struct Batch {
    hours: HashMap<HourKey, HourCounters>,
    paths: HashMap<PathKey, PathCounters>,
    totals: Totals,
    /// Distinct-path counter per (day, client), for the cardinality guard.
    path_cardinality: HashMap<(i64, String), usize>,
}

impl Batch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a parse failure. The line is skipped but its bytes are still
    /// consumed by the caller — bad data must never stall the reader.
    pub fn add_parse_error(&mut self) {
        self.totals.parse_errors += 1;
    }

    /// Fold one event into the batch.
    pub fn add(&mut self, ev: &Event<'_>) {
        let c = classify(ev.uri);
        self.totals.lines_ingested += 1;
        if c.decode_failed {
            self.totals.decode_failures += 1;
        }

        let hk = HourKey {
            hour_ts: hour_ts(ev.msec),
            client_ip: ev.client_ip.to_string(),
            repo: c.repo.clone(),
            kind: c.kind.as_str(),
        };
        let h = self.hours.entry(hk).or_default();

        let body = ev.body_bytes as i64;
        let upstream = ev.upstream_bytes as i64;
        let saved = ev.saved() as i64;

        match ev.cache {
            CacheClass::Hit => {
                h.req_hit += 1;
                h.bytes_hit += body;
                h.bytes_upstream_hit += upstream;
            }
            CacheClass::Miss => {
                h.req_miss += 1;
                h.bytes_miss += body;
            }
            CacheClass::Bypass => {
                h.req_bypass += 1;
                h.bytes_bypass += body;
            }
            CacheClass::None => {
                h.req_none += 1;
                h.bytes_none += body;
            }
        }
        h.bytes_upstream += upstream;
        h.time_sum_ms += i64::from(ev.request_ms);
        if ev.status >= 400 {
            h.req_err += 1;
        }
        if ev.is_head() {
            h.req_head += 1;
        }

        self.totals.reqs += 1;
        self.totals.bytes_served += body;
        self.totals.bytes_upstream += upstream;
        self.totals.bytes_saved += saved;
        match c.kind {
            Kind::Package => {
                self.totals.reqs_package += 1;
                self.totals.bytes_served_package += body;
            }
            Kind::Metadata => {
                self.totals.reqs_metadata += 1;
                self.totals.bytes_served_metadata += body;
            }
            Kind::Other => {}
        }

        // ---- path table ----------------------------------------------------
        // Errors get no path row at all: a 404 storm has zero analytical value
        // and is the realistic way this table blows up.
        if ev.status >= 400 || c.kind == Kind::Other {
            return;
        }

        let day = day_ts(ev.msec);
        let card_key = (day, ev.client_ip.to_string());
        let seen = self.path_cardinality.entry(card_key).or_insert(0);

        let pk = PathKey {
            day_ts: day,
            client_ip: ev.client_ip.to_string(),
            repo: c.repo,
            kind: c.kind.as_str(),
            path: c.path,
        };
        // Only count a NEW path against the cap; repeats of a known path are free.
        let key = if self.paths.contains_key(&pk) {
            pk
        } else if *seen >= MAX_PATHS_PER_CLIENT_DAY {
            PathKey {
                path: OTHER_PATH.to_string(),
                ..pk
            }
        } else {
            *seen += 1;
            pk
        };

        let p = self.paths.entry(key).or_default();
        p.reqs += 1;
        p.bytes += body;
        if ev.cache.is_hit() {
            p.req_hit += 1;
            p.bytes_hit += body;
            p.bytes_upstream_hit += upstream;
        }
        p.last_ts = p.last_ts.max(ev.msec as i64);
    }

    pub fn is_empty(&self) -> bool {
        self.hours.is_empty() && self.paths.is_empty() && self.totals.lines_ingested == 0
    }

    pub fn totals(&self) -> &Totals {
        &self.totals
    }

    /// Consume the batch, yielding rows ready to UPSERT.
    pub fn drain(self) -> Drained {
        (
            self.hours.into_iter().collect(),
            self.paths.into_iter().collect(),
            self.totals,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line::parse_line_at;

    const NOW: f64 = 1_784_418_424.0;

    fn ev(uri: &str, status: u16, body: u64, upstream: &str, cache: &str) -> String {
        format!(
            "1784418424.0\t192.168.10.10\tGET\t{status}\t{body}\t{upstream}\t{cache}\t0.1\t{uri}"
        )
    }

    fn batch_of(lines: &[String]) -> Batch {
        let mut b = Batch::new();
        for l in lines {
            match parse_line_at(l, NOW) {
                Ok(e) => b.add(&e),
                Err(_) => b.add_parse_error(),
            }
        }
        b
    }

    #[test]
    fn hour_and_day_buckets_floor_to_utc() {
        assert_eq!(hour_ts(1_784_418_424.9), 1_784_415_600);
        assert_eq!(day_ts(1_784_418_424.9), 1_784_332_800);
        assert_eq!(hour_ts(1_784_415_600.0), 1_784_415_600, "exact boundary");
        // Every bucket must be an exact multiple of its width, or the series
        // gains phantom points that no other bucket can line up with.
        assert_eq!(hour_ts(1_784_418_424.9) % 3600, 0);
        assert_eq!(day_ts(1_784_418_424.9) % 86_400, 0);
    }

    #[test]
    fn a_hit_contributes_its_whole_body_to_saved() {
        let b = batch_of(&[ev(
            "/debian/pool/main/c/c/cowsay_1_all.deb",
            200,
            1000,
            "-",
            "HIT",
        )]);
        assert_eq!(b.totals().bytes_saved, 1000);
        assert_eq!(b.totals().bytes_upstream, 0);
    }

    #[test]
    fn a_miss_contributes_zero_saved_even_when_upstream_exceeds_body() {
        // The live-measured case: upstream includes headers, body does not.
        let b = batch_of(&[ev(
            "/debian/dists/trixie/InRelease",
            200,
            5966,
            "6499",
            "MISS",
        )]);
        assert_eq!(b.totals().bytes_saved, 0, "never negative");
        assert_eq!(b.totals().bytes_upstream, 6499);
    }

    #[test]
    fn saved_is_reconstructible_from_the_stored_hit_columns() {
        // This is the reason bytes_upstream_hit exists as its own column.
        let b = batch_of(&[
            ev("/debian/a/x_1_all.deb", 200, 1000, "-", "HIT"),
            ev(
                "/debian/dists/trixie/InRelease",
                200,
                800,
                "300",
                "REVALIDATED",
            ),
            ev("/debian/b/y_1_all.deb", 200, 5000, "5200", "MISS"),
        ]);
        let totals_saved = b.totals().bytes_saved;
        let (hours, _, _) = b.drain();
        let derived: i64 = hours
            .iter()
            .map(|(_, c)| (c.bytes_hit - c.bytes_upstream_hit).max(0))
            .sum();
        assert_eq!(derived, totals_saved);
        assert_eq!(derived, 1000 + 500, "full HIT body + REVALIDATED net");
    }

    #[test]
    fn package_and_metadata_are_counted_separately() {
        let b = batch_of(&[
            ev(
                "/debian/pool/main/c/c/cowsay_1_all.deb",
                200,
                100,
                "-",
                "HIT",
            ),
            ev("/debian/dists/trixie/InRelease", 200, 200, "-", "HIT"),
        ]);
        assert_eq!(b.totals().reqs_package, 1);
        assert_eq!(b.totals().reqs_metadata, 1);
        assert_eq!(b.totals().bytes_served_package, 100);
        assert_eq!(b.totals().bytes_served_metadata, 200);
    }

    #[test]
    fn an_error_response_gets_no_path_row_but_still_counts_in_the_hour() {
        let b = batch_of(&[ev("/debian/nope.deb", 404, 196, "659", "MISS")]);
        let (hours, paths, _) = b.drain();
        assert_eq!(paths.len(), 0, "404s must not pollute the path table");
        assert_eq!(hours.len(), 1);
        assert_eq!(hours[0].1.req_err, 1);
    }

    #[test]
    fn the_banner_is_other_and_never_reaches_the_path_table() {
        let b = batch_of(&[ev("/", 200, 61, "-", "-")]);
        let (hours, paths, t) = b.drain();
        assert_eq!(paths.len(), 0);
        assert_eq!(hours[0].1.req_none, 1);
        assert_eq!(t.reqs_package + t.reqs_metadata, 0);
    }

    #[test]
    fn repeat_requests_for_one_path_accumulate_into_one_row() {
        let b = batch_of(&[
            ev("/debian/a/x_1_all.deb", 200, 100, "-", "HIT"),
            ev("/debian/a/x_1_all.deb", 200, 100, "-", "HIT"),
            ev("/debian/a/x_1_all.deb", 200, 100, "-", "HIT"),
        ]);
        let (_, paths, _) = b.drain();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].1.reqs, 3);
        assert_eq!(paths[0].1.bytes, 300);
    }

    #[test]
    fn encoded_and_plain_forms_of_one_package_share_a_row() {
        let b = batch_of(&[
            ev(
                "/debian/a/librbd1_18.2.7%2bds-1_amd64.deb",
                200,
                100,
                "-",
                "HIT",
            ),
            ev(
                "/debian/a/librbd1_18.2.7+ds-1_amd64.deb",
                200,
                100,
                "-",
                "HIT",
            ),
        ]);
        let (_, paths, _) = b.drain();
        assert_eq!(paths.len(), 1, "decoding must collapse these");
        assert_eq!(paths[0].1.reqs, 2);
    }

    #[test]
    fn distinct_paths_beyond_the_cap_fold_into_other() {
        let mut b = Batch::new();
        for i in 0..(MAX_PATHS_PER_CLIENT_DAY + 25) {
            let l = ev(
                &format!("/debian/pool/p{i}/pkg{i}_1_all.deb"),
                200,
                10,
                "-",
                "HIT",
            );
            b.add(&parse_line_at(&l, NOW).unwrap());
        }
        let (_, paths, _) = b.drain();
        assert_eq!(
            paths.len(),
            MAX_PATHS_PER_CLIENT_DAY + 1,
            "capped + one (other)"
        );
        let other = paths.iter().find(|(k, _)| k.path == OTHER_PATH).unwrap();
        assert_eq!(other.1.reqs, 25);
    }

    #[test]
    fn a_parse_error_is_counted_and_does_not_abort_the_batch() {
        let b = batch_of(&[
            ev("/debian/a/x_1_all.deb", 200, 100, "-", "HIT"),
            "garbage".to_string(),
            ev("/debian/a/y_1_all.deb", 200, 100, "-", "HIT"),
        ]);
        assert_eq!(b.totals().parse_errors, 1);
        assert_eq!(b.totals().lines_ingested, 2);
    }

    #[test]
    fn head_requests_are_tracked_separately() {
        let mut b = Batch::new();
        let l = "1784418424.0\t1.2.3.4\tHEAD\t200\t0\t-\tHIT\t0.0\t/debian/a/x_1_all.deb";
        b.add(&parse_line_at(l, NOW).unwrap());
        let (hours, _, _) = b.drain();
        assert_eq!(hours[0].1.req_head, 1);
        assert_eq!(hours[0].1.req_hit, 1, "still a hit, just a zero-byte one");
        assert_eq!(hours[0].1.bytes_hit, 0);
    }

    #[test]
    fn events_in_different_hours_land_in_different_buckets() {
        let mut b = Batch::new();
        for t in ["1784418424.0", "1784415000.0"] {
            let l = format!("{t}\t1.2.3.4\tGET\t200\t10\t-\tHIT\t0.0\t/debian/a/x_1_all.deb");
            b.add(&parse_line_at(&l, NOW).unwrap());
        }
        let (hours, paths, _) = b.drain();
        assert_eq!(hours.len(), 2, "two hourly buckets");
        assert_eq!(paths.len(), 1, "but the same day, so one path row");
    }

    #[test]
    fn an_empty_batch_reports_itself_empty() {
        assert!(Batch::new().is_empty());
    }
}
