//! Data sources: Prometheus job counters (exact) and the REST queue API (backlog drain).

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::prom;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SourceKind {
    /// Metrics when reachable, otherwise REST drain.
    Auto,
    Metrics,
    Rest,
}

/// Where the `done` counter of a sample came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Metrics,
    Drain,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Backlog {
    pub active: u64,
    pub waiting: u64,
    pub delayed: u64,
    pub paused: u64,
    pub failed: u64,
}

impl Backlog {
    /// Jobs still to run (paused jobs don't drain, so they're excluded).
    pub fn pending(&self) -> u64 {
        self.active + self.waiting + self.delayed
    }

    fn add(&mut self, o: &Backlog) {
        self.active += o.active;
        self.waiting += o.waiting;
        self.delayed += o.delayed;
        self.paused += o.paused;
        self.failed += o.failed;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct QueueState {
    pub backlog: Backlog,
    pub is_paused: bool,
}

#[derive(Debug, Default)]
pub struct Sample {
    /// Monotonic "jobs done" counter, when any source produced one.
    pub done: Option<(Mode, u64)>,
    pub per_job: BTreeMap<String, prom::JobCount>,
    /// Sum over `queues`.
    pub queue: Option<QueueState>,
    /// Every queue REST reported (just `--queue` when set).
    pub queues: BTreeMap<String, QueueState>,
    /// Per-queue drain counters (see [`Drain`]).
    pub per_queue_done: BTreeMap<String, u64>,
    pub errors: Vec<String>,
}

pub struct Config {
    pub kind: SourceKind,
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub metrics_url: Option<String>,
    pub queue: Option<String>,
    pub job: Option<String>,
}

/// Synthesizes a "done" counter from a draining backlog: each poll adds whatever the
/// pending count dropped by, plus new failures. Over-counts nothing, but under-counts
/// whenever jobs are being enqueued at the same time.
#[derive(Debug, Default)]
pub struct Drain {
    prev: Option<Backlog>,
    total: u64,
}

impl Drain {
    pub fn update(&mut self, cur: Backlog) -> u64 {
        if let Some(prev) = self.prev {
            self.total += prev.pending().saturating_sub(cur.pending());
            self.total += cur.failed.saturating_sub(prev.failed);
        }
        self.prev = Some(cur);
        self.total
    }
}

/// Sum of all queue states; paused only if every queue is.
pub fn aggregate(queues: &BTreeMap<String, QueueState>) -> QueueState {
    let mut backlog = Backlog::default();
    queues.values().for_each(|q| backlog.add(&q.backlog));
    QueueState { backlog, is_paused: !queues.is_empty() && queues.values().all(|q| q.is_paused) }
}

/// Keep only `want` (all when `None`), erroring with the known names on a miss.
fn select(mut all: BTreeMap<String, QueueState>, want: Option<&str>) -> Result<BTreeMap<String, QueueState>> {
    let Some(name) = want else { return Ok(all) };
    match all.remove(name) {
        Some(q) => Ok(BTreeMap::from([(name.to_string(), q)])),
        None => bail!("queue '{name}' not found; known: {}", all.keys().cloned().collect::<Vec<_>>().join(", ")),
    }
}

/// After this many consecutive metrics failures in auto mode, stop trying metrics.
const METRICS_GIVE_UP: u32 = 3;

pub struct Poller {
    cfg: Config,
    client: Client,
    metrics_failures: u32,
    drains: BTreeMap<String, Drain>,
    /// Whether the new `/api/queues` endpoint exists (None until probed).
    new_api: Option<bool>,
}

#[derive(Deserialize)]
struct QueueResponse {
    name: String,
    #[serde(rename = "isPaused")]
    is_paused: bool,
    statistics: Backlog,
}

#[derive(Deserialize)]
struct LegacyQueue {
    #[serde(rename = "jobCounts")]
    job_counts: Backlog,
    #[serde(rename = "queueStatus")]
    queue_status: LegacyStatus,
}

#[derive(Deserialize)]
struct LegacyStatus {
    #[serde(rename = "isPaused")]
    is_paused: bool,
}

impl Poller {
    pub fn new(cfg: Config, timeout: Duration) -> Result<Self> {
        let client = Client::builder().timeout(timeout).build()?;
        Ok(Self { cfg, client, metrics_failures: 0, drains: BTreeMap::new(), new_api: None })
    }

    fn metrics_enabled(&self) -> bool {
        match self.cfg.kind {
            SourceKind::Rest => false,
            SourceKind::Metrics => true,
            SourceKind::Auto => self.metrics_failures < METRICS_GIVE_UP,
        }
    }

    pub fn poll(&mut self) -> Sample {
        let mut sample = Sample::default();

        if self.metrics_enabled() {
            match self.poll_metrics() {
                Ok(per_job) => {
                    self.metrics_failures = 0;
                    let done = per_job.values().map(|c| c.done()).sum();
                    sample.done = Some((Mode::Metrics, done));
                    sample.per_job = per_job;
                }
                Err(e) => {
                    self.metrics_failures += 1;
                    let note = if self.cfg.kind == SourceKind::Auto && !self.metrics_enabled() {
                        " (giving up, using REST drain)"
                    } else {
                        ""
                    };
                    sample.errors.push(format!("metrics: {e:#}{note}"));
                }
            }
        }

        if self.cfg.api_key.is_some() && self.cfg.url.is_some() {
            match self.poll_queues().and_then(|all| select(all, self.cfg.queue.as_deref())) {
                Ok(queues) => {
                    // Summing per-queue drains keeps one queue filling up (e.g. thumbnails fed by
                    // metadata extraction) from cancelling out another one draining.
                    let mut drained = 0;
                    for (name, q) in &queues {
                        let done = self.drains.entry(name.clone()).or_default().update(q.backlog);
                        sample.per_queue_done.insert(name.clone(), done);
                        drained += done;
                    }
                    if sample.done.is_none() && self.cfg.kind != SourceKind::Metrics {
                        sample.done = Some((Mode::Drain, drained));
                    }
                    sample.queue = Some(aggregate(&queues));
                    sample.queues = queues;
                }
                Err(e) => sample.errors.push(format!("rest: {e:#}")),
            }
        }

        sample
    }

    fn poll_metrics(&self) -> Result<BTreeMap<String, prom::JobCount>> {
        let url = self.cfg.metrics_url.as_deref().context("no metrics URL")?;
        let text = self.client.get(url).send()?.error_for_status()?.text()?;
        let per_job = prom::job_counts(&text, self.cfg.job.as_deref());
        if per_job.is_empty() {
            match &self.cfg.job {
                Some(f) => bail!("no immich_jobs_* counters matching '{f}' yet (none run since start, or IMMICH_TELEMETRY_INCLUDE lacks 'job'?)"),
                None => bail!("no immich_jobs_* counters (is IMMICH_TELEMETRY_INCLUDE=job set? is this the microservices port 8082?)"),
            }
        }
        Ok(per_job)
    }

    fn get(&self, path: &str) -> Result<reqwest::blocking::Response> {
        let base = self.cfg.url.as_deref().context("no URL")?.trim_end_matches('/');
        let key = self.cfg.api_key.as_deref().context("no API key")?;
        Ok(self.client.get(format!("{base}{path}")).header("x-api-key", key).send()?)
    }

    /// Every queue's state, via `/api/queues` or (older servers) the legacy `/api/jobs`.
    fn poll_queues(&mut self) -> Result<BTreeMap<String, QueueState>> {
        if self.new_api != Some(false) {
            let resp = self.get("/api/queues")?;
            if resp.status() == StatusCode::NOT_FOUND && self.new_api.is_none() && self.probe_legacy()? {
                self.new_api = Some(false);
            } else {
                self.new_api = Some(true);
                let qs: Vec<QueueResponse> = resp.error_for_status()?.json()?;
                return Ok(qs
                    .into_iter()
                    .map(|q| (q.name, QueueState { backlog: q.statistics, is_paused: q.is_paused }))
                    .collect());
            }
        }
        let all: BTreeMap<String, LegacyQueue> = self.get("/api/jobs")?.error_for_status()?.json()?;
        Ok(all
            .into_iter()
            .map(|(name, q)| (name, QueueState { backlog: q.job_counts, is_paused: q.queue_status.is_paused }))
            .collect())
    }

    fn probe_legacy(&self) -> Result<bool> {
        Ok(self.get("/api/jobs")?.status().is_success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(waiting: u64, active: u64, failed: u64) -> Backlog {
        Backlog { waiting, active, failed, ..Default::default() }
    }

    #[test]
    fn drain_counts_backlog_drops_and_failures() {
        let mut d = Drain::default();
        assert_eq!(d.update(b(100, 4, 0)), 0);
        assert_eq!(d.update(b(90, 4, 0)), 10);
        assert_eq!(d.update(b(85, 4, 2)), 17); // 5 drained + 2 newly failed
        // Enqueue burst: backlog grows, counter holds (under-counts, never negative).
        assert_eq!(d.update(b(500, 4, 2)), 17);
        assert_eq!(d.update(b(495, 4, 2)), 22);
    }

    fn qs(entries: &[(&str, u64)]) -> BTreeMap<String, QueueState> {
        entries.iter().map(|&(n, w)| (n.to_string(), QueueState { backlog: b(w, 0, 0), is_paused: false })).collect()
    }

    #[test]
    fn per_queue_drains_dont_cancel() {
        // metadata drains 10 while thumbnails grows 10: summed backlog is flat, real work is 10.
        let mut drains: BTreeMap<String, Drain> = BTreeMap::new();
        let mut total = |all: BTreeMap<String, QueueState>| -> u64 {
            all.iter().map(|(n, q)| drains.entry(n.clone()).or_default().update(q.backlog)).sum()
        };
        assert_eq!(total(qs(&[("metadataExtraction", 100), ("thumbnailGeneration", 0)])), 0);
        assert_eq!(total(qs(&[("metadataExtraction", 90), ("thumbnailGeneration", 10)])), 10);
        assert_eq!(aggregate(&qs(&[("a", 90), ("b", 10)])).backlog.pending(), 100);
    }

    #[test]
    fn select_filters_queue() {
        let all = qs(&[("ocr", 5), ("sidecar", 7)]);
        assert_eq!(select(all.clone(), None).unwrap().len(), 2);
        let one = select(all.clone(), Some("ocr")).unwrap();
        assert_eq!(one.keys().collect::<Vec<_>>(), ["ocr"]);
        let err = select(all, Some("nope")).unwrap_err().to_string();
        assert!(err.contains("ocr, sidecar"), "{err}");
    }

    #[test]
    fn deserializes_queue_dto() {
        let json = r#"{"name":"thumbnailGeneration","isPaused":false,
            "statistics":{"active":3,"completed":0,"failed":1,"delayed":0,"waiting":42,"paused":0}}"#;
        let q: QueueResponse = serde_json::from_str(json).unwrap();
        assert!(!q.is_paused);
        assert_eq!(q.statistics.pending(), 45);
        assert_eq!(q.statistics.failed, 1);
    }

    #[test]
    fn deserializes_legacy_jobs_dto() {
        let json = r#"{"thumbnailGeneration":{"jobCounts":{"active":1,"completed":0,"failed":0,"delayed":0,"waiting":9,"paused":0},
            "queueStatus":{"isActive":true,"isPaused":true}}}"#;
        let all: BTreeMap<String, LegacyQueue> = serde_json::from_str(json).unwrap();
        let q = &all["thumbnailGeneration"];
        assert_eq!(q.job_counts.pending(), 10);
        assert!(q.queue_status.is_paused);
    }
}
