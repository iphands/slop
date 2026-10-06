//! Minimal Prometheus text-format parser, just enough for Immich's job counters.
//!
//! Immich (telemetry.service.ts) increments `immich.jobs.<snake_job>.<status>` once per job;
//! the OTel Prometheus exporter renders that as `immich_jobs_<job>_<status>_total`.

use std::collections::BTreeMap;

const STATUSES: [&str; 3] = ["success", "skipped", "failed"];

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct JobCount {
    pub success: u64,
    pub skipped: u64,
    pub failed: u64,
}

impl JobCount {
    /// Every job that left the queue, whatever its outcome.
    pub fn done(&self) -> u64 {
        self.success + self.skipped + self.failed
    }
}

/// Parse sample lines into `(metric_name, value)`, dropping labels, comments and junk.
pub fn parse(text: &str) -> Vec<(&str, f64)> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<(&str, f64)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let name_end = line.find(|c: char| c == '{' || c.is_whitespace())?;
    let name = &line[..name_end];
    let mut rest = &line[name_end..];
    if rest.starts_with('{') {
        rest = &rest[labels_end(rest)?..];
    }
    let value = rest.split_whitespace().next()?.parse::<f64>().ok()?;
    value.is_finite().then_some((name, value))
}

/// Byte index just past the `}` closing a label set, honoring quoted values.
fn labels_end(s: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if in_quotes => escaped = true,
            '"' => in_quotes = !in_quotes,
            '}' if !in_quotes => return Some(i + 1),
            _ => {}
        }
    }
    None
}

/// Split `immich_jobs_<job>_<status>[_total]` into `(job, status)`.
fn split_job_metric(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("immich_jobs_")?;
    let rest = rest.strip_suffix("_total").unwrap_or(rest);
    STATUSES.iter().find_map(|status| {
        let job = rest.strip_suffix(status)?.strip_suffix('_')?;
        (!job.is_empty()).then_some((job, *status))
    })
}

/// Per-job counters whose job name contains `filter` (all jobs when `None`).
pub fn job_counts(text: &str, filter: Option<&str>) -> BTreeMap<String, JobCount> {
    let mut out: BTreeMap<String, JobCount> = BTreeMap::new();
    for (name, value) in parse(text) {
        let Some((job, status)) = split_job_metric(name) else { continue };
        if filter.is_some_and(|f| !job.contains(f)) {
            continue;
        }
        let entry = out.entry(job.to_string()).or_default();
        let v = value.max(0.0) as u64;
        match status {
            "success" => entry.success += v,
            "skipped" => entry.skipped += v,
            _ => entry.failed += v,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
# HELP immich_jobs_asset_generate_thumbnails_success_total
# TYPE immich_jobs_asset_generate_thumbnails_success_total counter
immich_jobs_asset_generate_thumbnails_success_total{otel_scope_name="immich",x="a}b \"q\""} 120 1700000000
immich_jobs_asset_generate_thumbnails_skipped_total 3
immich_jobs_asset_generate_thumbnails_failed_total{otel_scope_name="immich"} 2
immich_jobs_asset_extract_metadata_success_total 7.0
immich_jobs_sidecar_check_success_total NaN
immich_queues_thumbnail_generation_active 4
process_cpu_seconds_total 1.5
garbage line
"#;

    #[test]
    fn parses_lines_with_labels_and_comments() {
        let samples = parse(FIXTURE);
        assert!(samples.contains(&("immich_jobs_asset_generate_thumbnails_success_total", 120.0)));
        assert!(samples.contains(&("immich_queues_thumbnail_generation_active", 4.0)));
        assert!(!samples.iter().any(|(n, _)| n.starts_with('#') || *n == "garbage"));
        // NaN is dropped
        assert!(!samples.iter().any(|(n, _)| n.contains("sidecar")));
    }

    #[test]
    fn splits_job_metric_names() {
        assert_eq!(
            split_job_metric("immich_jobs_asset_generate_thumbnails_failed_total"),
            Some(("asset_generate_thumbnails", "failed"))
        );
        assert_eq!(split_job_metric("immich_jobs_foo_success"), Some(("foo", "success")));
        assert_eq!(split_job_metric("immich_jobs_success_total"), None);
        assert_eq!(split_job_metric("immich_queues_foo_active"), None);
    }

    #[test]
    fn sums_job_counts_with_filter() {
        let all = job_counts(FIXTURE, None);
        let thumbs = all["asset_generate_thumbnails"];
        assert_eq!(thumbs, JobCount { success: 120, skipped: 3, failed: 2 });
        assert_eq!(thumbs.done(), 125);
        assert_eq!(all["asset_extract_metadata"].done(), 7);

        let filtered = job_counts(FIXTURE, Some("thumb"));
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains_key("asset_generate_thumbnails"));
    }
}
