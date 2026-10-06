mod prom;
mod source;
mod stats;
mod ui;

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Parser;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use source::{Config, Mode, Poller, QueueState, Sample, SourceKind};
use stats::Stats;

/// Live jobs/sec graph for Immich job queues.
#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    /// Immich base URL, e.g. http://nas:2283 (needed for REST and the default metrics URL)
    #[arg(long, env = "IMMICH_URL")]
    url: Option<String>,

    /// Admin API key (needs queue.read); enables queue depth / ETA and REST drain mode
    #[arg(long, env = "IMMICH_API_KEY", hide_env_values = true)]
    api_key: Option<String>,

    /// Prometheus endpoint of the microservices worker [default: <url host>:8082/metrics]
    #[arg(long, env = "IMMICH_METRICS_URL")]
    metrics_url: Option<String>,

    /// Poll interval, e.g. 1s, 5s, 500ms
    #[arg(long, default_value = "1s", value_parser = parse_duration)]
    interval: Duration,

    /// REST queue name, e.g. thumbnailGeneration (all queues summed when omitted)
    #[arg(long)]
    queue: Option<String>,

    /// Only count metric job names containing this, e.g. asset_generate_thumbnails
    #[arg(long)]
    job: Option<String>,

    /// Where the jobs-done counter comes from
    #[arg(long, value_enum, default_value_t = SourceKind::Auto)]
    source: SourceKind,

    /// How much history to keep, e.g. 10m
    #[arg(long, default_value = "10m", value_parser = parse_duration)]
    history: Duration,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().map_err(|_| format!("bad duration '{s}'"))?;
    let secs = match unit {
        "ms" => n / 1000.0,
        "" | "s" => n,
        "m" => n * 60.0,
        "h" => n * 3600.0,
        _ => return Err(format!("bad duration unit '{unit}' (use ms, s, m, h)")),
    };
    if secs <= 0.0 {
        return Err("duration must be > 0".into());
    }
    Ok(Duration::from_secs_f64(secs))
}

fn default_metrics_url(base: &str) -> Result<String> {
    let mut url = reqwest::Url::parse(base).with_context(|| format!("bad --url '{base}'"))?;
    url.set_port(Some(8082)).ok().context("URL cannot have a port")?;
    url.set_path("/metrics");
    url.set_query(None);
    Ok(url.to_string())
}

/// Graph zoom levels in seconds.
const ZOOMS: [f64; 6] = [30.0, 60.0, 120.0, 300.0, 600.0, 1800.0];

pub struct App {
    pub start: Instant,
    pub history: f64,
    pub interval: Duration,
    pub stats: Stats,
    pub job_stats: BTreeMap<String, Stats>,
    pub mode: Option<Mode>,
    pub queue: Option<QueueState>,
    pub errors: Vec<String>,
    pub last_poll: Option<Instant>,
    pub view: f64,
    failed_base: Option<u64>,
    pub failed: u64,
    pub target: String,
}

impl App {
    fn new(args: &Args) -> Self {
        let history = args.history.as_secs_f64();
        let mut target = Vec::new();
        if let Some(j) = &args.job {
            target.push(format!("job~{j}"));
        }
        if let Some(q) = &args.queue {
            target.push(format!("queue={q}"));
        }
        let target = if target.is_empty() { "all jobs".into() } else { target.join(" ") };
        let view = *ZOOMS.iter().find(|&&z| z >= 120.0_f64.min(history)).unwrap_or(&ZOOMS[0]);
        Self {
            start: Instant::now(),
            history,
            interval: args.interval,
            stats: Stats::new(history),
            job_stats: BTreeMap::new(),
            mode: None,
            queue: None,
            errors: Vec::new(),
            last_poll: None,
            view,
            failed_base: None,
            failed: 0,
            target,
        }
    }

    fn reset(&mut self) {
        self.stats = Stats::new(self.history);
        self.job_stats.clear();
        self.failed_base = None;
        self.failed = 0;
    }

    fn ingest(&mut self, at: Instant, s: Sample) {
        let t = at.duration_since(self.start).as_secs_f64();
        self.last_poll = Some(at);
        self.errors = s.errors;
        self.queue = s.queue;

        let Some((mode, done)) = s.done else { return };
        if self.mode != Some(mode) {
            // Counters from different sources aren't comparable.
            self.reset();
            self.mode = Some(mode);
        }
        self.stats.push(t, done);
        for (job, count) in &s.per_job {
            self.job_stats.entry(job.clone()).or_insert_with(|| Stats::new(self.history)).push(t, count.done());
        }

        let failed_now = match mode {
            Mode::Metrics => s.per_job.values().map(|c| c.failed).sum(),
            Mode::Drain => s.queue.map_or(0, |q| q.backlog.failed),
        };
        let base = *self.failed_base.get_or_insert(failed_now);
        if failed_now < base {
            self.failed_base = Some(failed_now);
        }
        self.failed = failed_now.saturating_sub(self.failed_base.unwrap_or(base));
    }

    fn zoom(&mut self, dir: i32) {
        let i = ZOOMS.iter().position(|&z| z == self.view).unwrap_or(2) as i32 + dir;
        let max = ZOOMS.iter().rposition(|&z| z <= self.history).unwrap_or(0) as i32;
        self.view = ZOOMS[i.clamp(0, max) as usize];
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let metrics_url = match (&args.metrics_url, &args.url) {
        (Some(m), _) => Some(m.clone()),
        (None, Some(u)) if args.source != SourceKind::Rest => Some(default_metrics_url(u)?),
        _ => None,
    };
    let has_rest = args.url.is_some() && args.api_key.is_some();
    match args.source {
        SourceKind::Rest if !has_rest => bail!("--source rest needs --url and --api-key"),
        SourceKind::Metrics if metrics_url.is_none() => bail!("--source metrics needs --metrics-url or --url"),
        SourceKind::Auto if metrics_url.is_none() && !has_rest => {
            bail!("need --url (with --api-key for queue depth) or --metrics-url")
        }
        _ => {}
    }

    let mut poller = Poller::new(
        Config {
            kind: args.source,
            url: args.url.clone(),
            api_key: args.api_key.clone(),
            metrics_url,
            queue: args.queue.clone(),
            job: args.job.clone(),
        },
        args.interval.max(Duration::from_secs(2)),
    )?;

    let mut app = App::new(&args);
    let (tx, rx) = mpsc::channel();
    let interval = args.interval;
    thread::spawn(move || {
        loop {
            let started = Instant::now();
            let sample = poller.poll();
            if tx.send((started, sample)).is_err() {
                return;
            }
            thread::sleep(interval.saturating_sub(started.elapsed()));
        }
    });

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &rx);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App, rx: &mpsc::Receiver<(Instant, Sample)>) -> Result<()> {
    loop {
        while let Ok((at, sample)) = rx.try_recv() {
            app.ingest(at, sample);
        }
        terminal.draw(|f| ui::draw(f, app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(k) = event::read()?
            && k.kind == KeyEventKind::Press
        {
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                KeyCode::Char('r') => app.reset(),
                KeyCode::Char('+') | KeyCode::Char('=') => app.zoom(-1),
                KeyCode::Char('-') | KeyCode::Char('_') => app.zoom(1),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("1s").unwrap(), Duration::from_secs(1));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("10m").unwrap(), Duration::from_secs(600));
        assert_eq!(parse_duration("2").unwrap(), Duration::from_secs(2));
        assert_eq!(parse_duration("1.5s").unwrap(), Duration::from_millis(1500));
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("5d").is_err());
    }

    #[test]
    fn metrics_url_from_base() {
        assert_eq!(default_metrics_url("http://nas:2283").unwrap(), "http://nas:8082/metrics");
        assert_eq!(default_metrics_url("https://photos.example.com/").unwrap(), "https://photos.example.com:8082/metrics");
        assert_eq!(default_metrics_url("http://[::1]:2283/api").unwrap(), "http://[::1]:8082/metrics");
    }
}
