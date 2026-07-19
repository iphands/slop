//! Parsing one line of nginx's `log_format stats`.
//!
//! The format is 9 tab-separated fields, defined in `proxy/nginx.conf`:
//!
//! ```text
//! 1 msec  2 remote_addr  3 method  4 status  5 body_bytes
//! 6 upstream_bytes  7 cache_status  8 request_time  9 request_uri
//! ```
//!
//! Field 9 is `$request_uri` — the ORIGINAL, pre-`rewrite` URI. It is last
//! because it is the only variable-length, client-influenced field, so a
//! malformed one cannot shift the position of anything we care about.

use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

/// How a request was served, collapsed from nginx's `$upstream_cache_status`.
///
/// The grouping is what the dashboard's ratios are built on, so it is worth
/// stating precisely: `REVALIDATED` counts as a hit because the body came from
/// disk (only a conditional round-trip went upstream), while `EXPIRED` counts as
/// a miss because the body was re-fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheClass {
    /// Served from cache: `HIT`, `REVALIDATED`, `STALE`, `UPDATING`.
    Hit,
    /// Fetched from upstream: `MISS`, `EXPIRED`.
    Miss,
    /// Cache deliberately skipped: `BYPASS`.
    Bypass,
    /// Not a proxied response at all — `-` or empty. The `/` banner and any
    /// error before proxying land here, and they are excluded from every ratio
    /// denominator.
    None,
}

impl CacheClass {
    /// Classify a raw `$upstream_cache_status` value.
    ///
    /// `-` and `""` are treated identically; nginx emits `-` for an unset
    /// variable but some paths yield an empty field, and conflating them is the
    /// single most likely source of a "why is my hit ratio 60%" bug report.
    pub fn parse(s: &str) -> Self {
        match s {
            "HIT" | "REVALIDATED" | "STALE" | "UPDATING" => Self::Hit,
            "MISS" | "EXPIRED" => Self::Miss,
            "BYPASS" => Self::Bypass,
            _ => Self::None,
        }
    }

    /// Whether this class counts toward the hit side of a ratio.
    pub fn is_hit(self) -> bool {
        matches!(self, Self::Hit)
    }

    /// Whether this class belongs in a ratio denominator at all.
    ///
    /// `None` does not: a request that never consulted the cache is not a cache
    /// miss, and counting it as one understates the cache.
    pub fn counts_in_ratio(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// One parsed access-log line. Borrows from the input buffer; nothing allocates.
#[derive(Debug, Clone, PartialEq)]
pub struct Event<'a> {
    /// Epoch seconds with millisecond precision, from `$msec`.
    ///
    /// This is the ONLY time source. The dated log filename is never parsed —
    /// see `context/distilled.md`, "the ingest never infers time from a
    /// filename".
    pub msec: f64,
    pub client_ip: &'a str,
    pub method: &'a str,
    pub status: u16,
    /// `$body_bytes_sent` — what the client received, excluding headers.
    pub body_bytes: u64,
    /// `$upstream_bytes_received` — what we actually paid for. Zero when `-`.
    ///
    /// On a MISS this can EXCEED `body_bytes`, because it counts response
    /// headers and `body_bytes` does not (measured live: 6499 vs 5966). That is
    /// why "bytes saved" is a hit-class-only subtraction; see [`Event::saved`].
    pub upstream_bytes: u64,
    pub cache: CacheClass,
    /// `$request_time` in milliseconds.
    pub request_ms: u32,
    /// `$request_uri` — original, pre-`rewrite`, still percent-encoded.
    pub uri: &'a str,
}

impl Event<'_> {
    /// Bytes this request saved us, per the formula corrected against live data.
    ///
    /// ```text
    /// saved = max(0, body_bytes - upstream_bytes)   for hit-class only
    ///       = 0                                     otherwise
    /// ```
    ///
    /// - `HIT` → `upstream_bytes == 0` → the whole body counts.
    /// - `REVALIDATED` → nets out the ~300-byte conditional round-trip.
    /// - `MISS` → contributes exactly 0, never a negative number.
    ///
    /// A whole-window `Σ served − Σ upstream` would instead charge every MISS's
    /// header overhead against the savings and can go negative on a quiet day.
    pub fn saved(&self) -> u64 {
        if self.cache.is_hit() {
            self.body_bytes.saturating_sub(self.upstream_bytes)
        } else {
            0
        }
    }

    /// HEAD responses carry a real cache status but zero bytes, so they inflate
    /// hit *counts* while contributing nothing to byte ratios. Our own
    /// `curl -sI` verification traffic is exactly this — a distortion that shows
    /// up only while you are testing, which is the worst time for it.
    pub fn is_head(&self) -> bool {
        self.method.eq_ignore_ascii_case("HEAD")
    }
}

/// Why a line was rejected. Every variant increments `parse_errors` and skips
/// the line — but the bytes are still consumed, so bad data can never stall the
/// reader.
// No `Eq`: MsecOutOfRange carries an f64, which is only PartialEq.
#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
    #[error("expected 9 tab-separated fields, got {0}")]
    FieldCount(usize),
    #[error("unparseable $msec: {0:?}")]
    BadMsec(String),
    #[error("$msec {0} outside the plausible window [now-90d, now+1d]")]
    MsecOutOfRange(f64),
    #[error("unparseable numeric field {field}: {value:?}")]
    BadNumber { field: &'static str, value: String },
}

/// 90 days back — generous enough for a long catch-up after downtime.
const MAX_AGE_SECS: f64 = 90.0 * 86_400.0;
/// 1 day forward — tolerates modest clock skew, rejects a broken clock.
const MAX_FUTURE_SECS: f64 = 86_400.0;

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Parse one line. `now` is injected so the range check is testable.
pub fn parse_line_at(line: &str, now: f64) -> Result<Event<'_>, ParseError> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);

    let mut it = line.split('\t');
    let mut f = [""; 9];
    let mut count = 0usize;
    for slot in f.iter_mut() {
        match it.next() {
            Some(v) => {
                *slot = v;
                count += 1;
            }
            None => break,
        }
    }
    // More than 9 fields is as wrong as fewer: it means the framing broke.
    if count != 9 || it.next().is_some() {
        let total = count + it.count() + usize::from(count == 9);
        return Err(ParseError::FieldCount(total.max(count)));
    }

    let msec: f64 = f[0]
        .parse()
        .map_err(|_| ParseError::BadMsec(f[0].to_string()))?;
    if !msec.is_finite() || msec < now - MAX_AGE_SECS || msec > now + MAX_FUTURE_SECS {
        // An NTP-less box booting at epoch 0 must not create an orphan bucket
        // 56 years in the past.
        return Err(ParseError::MsecOutOfRange(msec));
    }

    let status = num::<u16>(f[3], "status")?;
    let body_bytes = num::<u64>(f[4], "body_bytes")?;
    let upstream_bytes = sum_upstream_bytes(f[5])?;
    let request_ms = seconds_to_ms(f[7])?;

    Ok(Event {
        msec,
        client_ip: normalize_ip(f[1]),
        method: f[2],
        status,
        body_bytes,
        upstream_bytes,
        cache: CacheClass::parse(f[6]),
        request_ms,
        uri: f[8],
    })
}

/// Parse one line against the current clock.
pub fn parse_line(line: &str) -> Result<Event<'_>, ParseError> {
    parse_line_at(line, now_epoch())
}

/// Collapse an IPv4-mapped IPv6 address to its IPv4 form.
///
/// `proxy/conf.d/pkgcache.conf` has `listen [::]:8080`, so a dual-stack client
/// can arrive either way and would otherwise appear as two unrelated rows in the
/// client table.
fn normalize_ip(ip: &str) -> &str {
    ip.strip_prefix("::ffff:")
        .filter(|rest| rest.contains('.'))
        .unwrap_or(ip)
}

fn num<T: std::str::FromStr>(s: &str, field: &'static str) -> Result<T, ParseError> {
    s.parse().map_err(|_| ParseError::BadNumber {
        field,
        value: s.to_string(),
    })
}

/// Sum `$upstream_bytes_received`, which is **not always a single number**.
///
/// nginx documents the `$upstream_*` variables as carrying one value per
/// upstream connection, "separated by commas and colons like addresses in the
/// `$upstream_addr` variable". A request that hit more than one upstream —
/// a `proxy_next_upstream` retry, or an internal redirect — logs something like
/// `0, 908`.
///
/// Observed live 2026-07-18 on a 404 through `/debian/`, and it was rejecting
/// the entire line. Any such request would have vanished from the stats.
///
/// `-` and empty both mean zero (e.g. a cache HIT, which never went upstream).
fn sum_upstream_bytes(s: &str) -> Result<u64, ParseError> {
    if s == "-" || s.is_empty() {
        return Ok(0);
    }
    let mut total: u64 = 0;
    for part in s.split([',', ':']) {
        let p = part.trim();
        if p.is_empty() || p == "-" {
            continue; // one leg of a multi-upstream request contributed nothing
        }
        total = total.saturating_add(num::<u64>(p, "upstream_bytes")?);
    }
    Ok(total)
}

/// `$request_time` is fractional seconds ("0.266"); we store milliseconds.
fn seconds_to_ms(s: &str) -> Result<u32, ParseError> {
    if s == "-" || s.is_empty() {
        return Ok(0);
    }
    let secs: f64 = s.parse().map_err(|_| ParseError::BadNumber {
        field: "request_time",
        value: s.to_string(),
    })?;
    if !secs.is_finite() || secs < 0.0 {
        return Ok(0);
    }
    Ok((secs * 1000.0).round().min(f64::from(u32::MAX)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_784_418_424.0;

    fn line(fields: &[&str]) -> String {
        fields.join("\t")
    }

    /// A real line captured from the live proxy on 2026-07-18.
    fn real_hit() -> String {
        line(&[
            "1784418424.353",
            "172.17.0.1",
            "GET",
            "200",
            "140416",
            "-",
            "HIT",
            "0.000",
            "/debian/dists/trixie/InRelease",
        ])
    }

    #[test]
    fn a_real_hit_line_parses() {
        let s = real_hit();
        let ev = parse_line_at(&s, NOW).unwrap();
        assert_eq!(ev.client_ip, "172.17.0.1");
        assert_eq!(ev.status, 200);
        assert_eq!(ev.body_bytes, 140_416);
        assert_eq!(ev.upstream_bytes, 0, "'-' must parse as zero");
        assert_eq!(ev.cache, CacheClass::Hit);
        assert_eq!(ev.uri, "/debian/dists/trixie/InRelease");
    }

    #[test]
    fn a_real_miss_line_has_upstream_bytes_exceeding_body_bytes() {
        // Measured live: upstream counts response headers, body_bytes does not.
        let s = line(&[
            "1784418424.346",
            "172.17.0.1",
            "GET",
            "200",
            "140416",
            "141128",
            "MISS",
            "0.108",
            "/debian/dists/trixie/InRelease",
        ]);
        let ev = parse_line_at(&s, NOW).unwrap();
        assert!(ev.upstream_bytes > ev.body_bytes);
        assert_eq!(ev.saved(), 0, "a MISS must contribute 0, never a negative");
    }

    #[test]
    fn a_multi_upstream_bytes_field_is_summed_not_rejected() {
        // OBSERVED LIVE 2026-07-18. nginx logs one value per upstream connection,
        // comma/colon separated, when a request hits more than one upstream --
        // a proxy_next_upstream retry or an internal redirect. This exact line
        // was silently dropping a 404 from the stats until the awk-vs-sqlite
        // gate caught the one-line discrepancy.
        let s = line(&[
            "1784420365.440",
            "172.17.0.1",
            "GET",
            "404",
            "300",
            "0, 908",
            "MISS",
            "0.399",
            "/debian/does-not-exist.deb",
        ]);
        let ev = parse_line_at(&s, NOW).unwrap();
        assert_eq!(ev.upstream_bytes, 908, "0 + 908");
        assert_eq!(ev.status, 404);
    }

    #[test]
    fn upstream_bytes_separators_and_placeholders_are_handled() {
        assert_eq!(sum_upstream_bytes("-").unwrap(), 0);
        assert_eq!(sum_upstream_bytes("").unwrap(), 0);
        assert_eq!(sum_upstream_bytes("512").unwrap(), 512);
        assert_eq!(sum_upstream_bytes("0, 908").unwrap(), 908);
        // Colons appear for internal redirects, and a leg may be "-".
        assert_eq!(sum_upstream_bytes("100 : 200").unwrap(), 300);
        assert_eq!(sum_upstream_bytes("-, 42").unwrap(), 42);
        assert!(sum_upstream_bytes("nonsense").is_err());
    }

    #[test]
    fn a_line_with_eight_fields_is_a_parse_error() {
        let s = line(&[
            "1784418424.0",
            "1.2.3.4",
            "GET",
            "200",
            "1",
            "-",
            "HIT",
            "0.0",
        ]);
        assert_eq!(parse_line_at(&s, NOW), Err(ParseError::FieldCount(8)));
    }

    #[test]
    fn a_line_with_ten_fields_is_a_parse_error() {
        let s = line(&[
            "1784418424.0",
            "1.2.3.4",
            "GET",
            "200",
            "1",
            "-",
            "HIT",
            "0.0",
            "/a",
            "extra",
        ]);
        assert!(matches!(
            parse_line_at(&s, NOW),
            Err(ParseError::FieldCount(_))
        ));
    }

    #[test]
    fn every_cache_status_value_classifies() {
        for (raw, want) in [
            ("HIT", CacheClass::Hit),
            ("REVALIDATED", CacheClass::Hit),
            ("STALE", CacheClass::Hit),
            ("UPDATING", CacheClass::Hit),
            ("MISS", CacheClass::Miss),
            ("EXPIRED", CacheClass::Miss),
            ("BYPASS", CacheClass::Bypass),
            ("-", CacheClass::None),
            ("", CacheClass::None),
        ] {
            assert_eq!(CacheClass::parse(raw), want, "for {raw:?}");
        }
    }

    #[test]
    fn a_dash_and_an_empty_cache_status_are_treated_identically() {
        assert_eq!(CacheClass::parse("-"), CacheClass::parse(""));
        assert!(!CacheClass::parse("-").counts_in_ratio());
    }

    #[test]
    fn an_escaped_tab_in_the_uri_does_not_break_framing() {
        // escape=default renders a tab as \x09, and $request_uri keeps percent
        // encoding, so the field count survives a hostile request line.
        let s = line(&[
            "1784418424.0",
            "1.2.3.4",
            "GET",
            "404",
            "196",
            "659",
            "MISS",
            "0.268",
            "/debian/%09%22weird",
        ]);
        let ev = parse_line_at(&s, NOW).unwrap();
        assert_eq!(ev.uri, "/debian/%09%22weird");
    }

    #[test]
    fn an_out_of_range_msec_is_rejected() {
        let s = line(&["0.0", "1.2.3.4", "GET", "200", "1", "-", "HIT", "0.0", "/a"]);
        // A box booting at epoch 0 must not create a bucket in 1970.
        assert!(matches!(
            parse_line_at(&s, NOW),
            Err(ParseError::MsecOutOfRange(_))
        ));
    }

    #[test]
    fn a_far_future_msec_is_rejected() {
        let s = line(&[
            "9999999999.0",
            "1.2.3.4",
            "GET",
            "200",
            "1",
            "-",
            "HIT",
            "0.0",
            "/a",
        ]);
        assert!(matches!(
            parse_line_at(&s, NOW),
            Err(ParseError::MsecOutOfRange(_))
        ));
    }

    #[test]
    fn an_unparseable_msec_is_rejected() {
        let s = line(&[
            "not-a-number",
            "1.2.3.4",
            "GET",
            "200",
            "1",
            "-",
            "HIT",
            "0.0",
            "/a",
        ]);
        assert!(matches!(
            parse_line_at(&s, NOW),
            Err(ParseError::BadMsec(_))
        ));
    }

    #[test]
    fn ipv4_mapped_ipv6_collapses_to_ipv4() {
        // Otherwise one dual-stack laptop becomes two rows in the client table.
        assert_eq!(normalize_ip("::ffff:192.168.10.10"), "192.168.10.10");
        assert_eq!(normalize_ip("192.168.10.10"), "192.168.10.10");
        assert_eq!(normalize_ip("fe80::1"), "fe80::1", "real v6 is left alone");
    }

    #[test]
    fn a_head_request_is_flagged_and_carries_zero_bytes() {
        let s = line(&[
            "1784418424.4",
            "172.17.0.1",
            "HEAD",
            "200",
            "0",
            "-",
            "HIT",
            "0.000",
            "/debian/dists/trixie/InRelease",
        ]);
        let ev = parse_line_at(&s, NOW).unwrap();
        assert!(ev.is_head());
        assert_eq!(ev.body_bytes, 0);
    }

    #[test]
    fn request_time_becomes_milliseconds() {
        let s = line(&[
            "1784418424.0",
            "1.2.3.4",
            "GET",
            "200",
            "1",
            "-",
            "HIT",
            "0.266",
            "/a",
        ]);
        assert_eq!(parse_line_at(&s, NOW).unwrap().request_ms, 266);
    }

    #[test]
    fn a_hit_saves_its_whole_body() {
        let s = real_hit();
        assert_eq!(parse_line_at(&s, NOW).unwrap().saved(), 140_416);
    }

    #[test]
    fn a_revalidated_response_nets_out_its_conditional_round_trip() {
        let s = line(&[
            "1784418424.0",
            "1.2.3.4",
            "GET",
            "200",
            "140416",
            "300",
            "REVALIDATED",
            "0.05",
            "/a",
        ]);
        let ev = parse_line_at(&s, NOW).unwrap();
        assert_eq!(ev.saved(), 140_116, "full body minus the 304 round-trip");
    }

    #[test]
    fn a_trailing_newline_or_crlf_is_tolerated() {
        let base = real_hit();
        assert!(parse_line_at(&format!("{base}\n"), NOW).is_ok());
        assert!(parse_line_at(&format!("{base}\r\n"), NOW).is_ok());
    }
}
