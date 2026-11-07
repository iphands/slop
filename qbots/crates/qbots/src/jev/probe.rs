//! # qbots::jev::probe — `qbots jev-probe`, the only thing that talks to the live box in
//! development (Plan 72 T9)
//!
//! A `qbots` subcommand rather than a `crates/tools` binary: `qbots` is bin-only, so a tools
//! binary could not reuse the production client, renderer, validator or config — and every
//! number this tool prints must come from exactly the code the fleet runs.
//!
//! Live checks assert **shape and direction, never exact probabilities** (an exact assertion
//! false-alarms on a model bump and — worse — passes on a model that stopped reading the state).
//! Every request/response pair is appended to [`HARVEST`] for fixtures and for `replay`.
//! `matrix --live` (≈40 scenarios) is **cut** (plan cut order #2); `replay` is kept.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use brain::jev::decision::{Decision, TransportErr};
use brain::jev::gate::{stats, GateThresholds};
use brain::jev::question::{ChoiceOption, Phrase, QuestionKey, QuestionKind, QuestionSpec};
use brain::jev::state::{
    DecisionState, DistBand, DmgFacts, EnemyFacts, Field, HealthBand, ItemFacts, LoadoutGap,
    LocFacts, MapFacts, MapId, SelfFacts, SinceBand,
};
use brain::weapons::Weapon;
use clap::{Subcommand, ValueEnum};

use super::net::{post_json, Endpoint, PostError};
use super::render::{est_tokens, render_request, render_state, RenderedState};
use super::wire::{validate, QuestionResult};
use crate::config::Config;

/// Where every live request/response pair is appended (one JSON object per line).
pub const HARVEST: &str = "context/fixtures/jev/harvest.jsonl";
/// The single-stream throughput measured 2026-11-07 (the honest headroom figure).
const SINGLE_STREAM_TOK_S: f64 = 3900.0;
/// The 8-concurrent "hammered" figure (latency degraded to get it).
const HAMMERED_TOK_S: f64 = 6200.0;
/// Deadline for one-off probe asks (bench uses the fleet's own `deadline_ms`).
const PROBE_DEADLINE: Duration = Duration::from_secs(5);
/// A distribution "moved" if its L1 distance changed by at least this much.
const MOVED_L1: f32 = 0.05;

/// `qbots jev-probe <sub>` subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum ProbeCmd {
    /// Endpoint reachable, serving the configured model, and answering the chat route with the
    /// 501 capability fingerprint.
    Health,
    /// Ask one catalogue question about a state file (rendered-state text, one field per line).
    Ask {
        /// The state text file.
        #[arg(long)]
        state: PathBuf,
        /// Which catalogue question.
        #[arg(long, value_enum, default_value_t = ProbeQuestion::Posture)]
        question: ProbeQuestion,
    },
    /// Load test at the fleet's demand: p50/p95/p99, tok/s against both reference figures, and
    /// whether p95 fits `deadline_ms`. Defaults are derived from the live `jev:` config.
    Bench {
        /// Simulated bots (default: `fleet.count`, or 36 when that is 0).
        #[arg(long)]
        bots: Option<usize>,
        /// Requests per second (default: bots ÷ `min_dwell_s` — the refresh demand).
        #[arg(long)]
        rate: Option<f64>,
        /// Concurrent requests (default: `max_concurrent`).
        #[arg(long)]
        concurrent: Option<usize>,
        /// Duration in seconds.
        #[arg(long, default_value_t = 30)]
        secs: u64,
    },
    /// Choose gate thresholds from PRE-REGISTERED, human-labelled sets:
    /// `<dir>/<question>/obvious/<expected_label>/*.txt`, `<dir>/<question>/ambiguous/*.txt`,
    /// `<dir>/<question>/flat/*.txt` (≥10 obvious, ≥10 ambiguous). An obvious case counts only
    /// when the model picks its expected label.
    Calibrate {
        /// Root of the labelled sets.
        #[arg(long, default_value = "context/fixtures/jev/calibration")]
        dir: PathBuf,
    },
    /// Two states differing in one bucket must move the distribution (both catalogue pairs).
    Sensitivity,
    /// Two materially different states must not return identical distributions.
    Anticorrelation,
    /// Questions over meaningless options must be rejected by our gates.
    Flatness,
    /// Re-send every harvested request and diff what it said then vs now.
    Replay {
        /// The harvest file.
        #[arg(long, default_value = HARVEST)]
        file: PathBuf,
    },
    /// Plan 73 arena: analyse decision journals (offline — no model calls).
    Journal {
        /// Journal files (`logs/jev/journal-*.jsonl`).
        files: Vec<PathBuf>,
        /// Also write the worst outcomes as candidate scenarios (state text) here.
        #[arg(long)]
        candidates: Option<PathBuf>,
        /// How many candidates at most.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Plan 73 language lab: score closed wording/fact variants on the labelled corpus.
    Lab {
        /// Corpus root (`<dir>/<question>/{obvious/<label>,ambiguous,flat}/*.txt`).
        #[arg(long, default_value = "context/fixtures/jev/calibration")]
        dir: PathBuf,
        /// One question (default: both).
        #[arg(long, value_enum)]
        question: Option<ProbeQuestion>,
        /// One variant (default: all, baseline first).
        #[arg(long, value_enum)]
        variant: Option<super::lab::Variant>,
        /// Which corpus half to score.
        #[arg(long, value_enum, default_value_t = super::lab::Half::Tuning)]
        half: super::lab::Half,
        /// Required to score the holdout (spent once per final wording).
        #[arg(long)]
        confirm_holdout: bool,
        /// Only print the tuning/holdout counts (no model calls).
        #[arg(long)]
        split_report: bool,
        /// Also write one JSON line per scored case (name, kind, expected, top, margin,
        /// probabilities) to this file.
        #[arg(long)]
        detail: Option<PathBuf>,
    },
}

/// The probe's questions — the production ones from `brain::jev::question`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ProbeQuestion {
    /// Binary posture: disengage / hold.
    Posture,
    /// Three-way objective: seek health / seek armor / carry on (the no-op — Plan 73 round 3).
    Objective,
}

impl ProbeQuestion {
    /// The corpus sub-directory name (`posture`, `objective`).
    pub(crate) fn dir_name(self) -> &'static str {
        match self {
            ProbeQuestion::Posture => "posture",
            ProbeQuestion::Objective => "objective",
        }
    }

    /// The question spec — the production question (Plan 73 T4: one source for lab and bot).
    pub(crate) fn spec(self) -> QuestionSpec {
        match self {
            ProbeQuestion::Posture => brain::jev::question::posture_question(),
            ProbeQuestion::Objective => brain::jev::question::objective_question(),
        }
    }
}

/// The two catalogue situations sensitivity/anticorrelation are built from.
fn scenario(strong: bool, armor_near: bool) -> DecisionState {
    let (hp, armor, held, ammo) = if strong {
        (100, 100, Weapon::Railgun, 20)
    } else {
        (20, 0, Weapon::Blaster, 50)
    };
    DecisionState::from_fields(vec![
        Field::Self_(SelfFacts {
            hp,
            band: HealthBand::of(hp as i32),
            armor,
            held: Some(held),
            ammo,
            ammo_low: false,
        }),
        Field::Dmg(DmgFacts {
            recent: if strong { 0 } else { 80 },
            since: (!strong).then_some(SinceBand::Now),
        }),
        Field::Enemy(EnemyFacts {
            count: 2,
            nearest: Some(DistBand::Near),
            moving: true,
        }),
        Field::Loc(LocFacts {
            exits: Some(3),
            moving: true,
        }),
        Field::Items(ItemFacts {
            health: Some(DistBand::Mid),
            armor: Some(if armor_near {
                DistBand::Touch
            } else {
                DistBand::Far
            }),
            weapon: None,
            powerup: None,
        }),
        Field::Map(MapFacts {
            map: MapId::Q2dm1,
            gap: LoadoutGap::None,
        }),
    ])
}

/// Run a probe subcommand. `Ok(true)` = every check passed.
pub async fn run(cfg: &Config, cmd: ProbeCmd, json: bool) -> Result<bool, String> {
    let ep_str = std::env::var("JEV_ENDPOINT").unwrap_or_else(|_| cfg.jev.endpoint.clone());
    let p = Probe {
        ep: Endpoint::parse(&ep_str)?,
        model: cfg.jev.model.clone(),
        json,
        gate: GateThresholds {
            margin_binary: cfg.jev.gate_margin_binary,
            margin_multi: cfg.jev.gate_margin_multi,
            ..GateThresholds::default()
        },
    };
    match cmd {
        ProbeCmd::Health => p.health().await,
        ProbeCmd::Ask { state, question } => p.ask_file(&state, question).await,
        ProbeCmd::Bench {
            bots,
            rate,
            concurrent,
            secs,
        } => {
            let bots = bots.unwrap_or(if cfg.fleet.count == 0 {
                36
            } else {
                cfg.fleet.count
            });
            let rate = rate.unwrap_or_else(|| default_rate(bots, cfg.jev.min_dwell_s));
            let conc = concurrent.unwrap_or(cfg.jev.max_concurrent).max(1);
            let deadline = Duration::from_millis(cfg.jev.deadline_ms);
            p.bench(rate, conc, Duration::from_secs(secs), deadline)
                .await
        }
        ProbeCmd::Calibrate { dir } => p.calibrate(&dir).await,
        ProbeCmd::Sensitivity => p.sensitivity().await,
        ProbeCmd::Anticorrelation => p.anticorrelation().await,
        ProbeCmd::Flatness => p.flatness().await,
        ProbeCmd::Replay { file } => p.replay(&file).await,
        ProbeCmd::Journal {
            files,
            candidates,
            limit,
        } => journal_report(&files, candidates.as_deref(), limit),
        ProbeCmd::Lab {
            dir,
            question,
            variant,
            half,
            confirm_holdout,
            split_report,
            detail,
        } => {
            let qs = question.map_or_else(
                || vec![ProbeQuestion::Posture, ProbeQuestion::Objective],
                |q| vec![q],
            );
            if split_report {
                return lab_split_report(&dir, &qs);
            }
            super::lab::guard_half(half, confirm_holdout)?;
            let vs = variant.map_or_else(|| super::lab::Variant::ALL.to_vec(), |v| vec![v]);
            p.lab(&dir, &qs, &vs, half, detail.as_deref()).await
        }
    }
}

/// The refresh demand the fleet will generate: bots ÷ `min_dwell_s` requests per second.
pub fn default_rate(bots: usize, min_dwell_s: f64) -> f64 {
    bots as f64 / min_dwell_s.max(0.1)
}

/// One answer, kept for reporting.
struct Asked {
    probs: Vec<f32>,
    result: QuestionResult,
    input_tokens: Option<u32>,
    latency: Duration,
}

struct Probe {
    ep: Endpoint,
    model: String,
    json: bool,
    gate: GateThresholds,
}

impl Probe {
    /// POST a body, harvest the pair, return the 200 body.
    async fn post(&self, sub: &str, body: &str, deadline: Duration) -> Result<Vec<u8>, PostError> {
        let r = post_json(&self.ep, "/v1/systemone", body, deadline).await;
        harvest(sub, body, &r);
        r
    }

    /// Ask `q` about a rendered state; validate + gate with default thresholds.
    async fn ask(
        &self,
        sub: &str,
        state: &RenderedState,
        q: &QuestionSpec,
    ) -> Result<Asked, String> {
        let body = render_request(state, std::slice::from_ref(q));
        let t0 = Instant::now();
        let bytes = self
            .post(sub, &body, PROBE_DEADLINE)
            .await
            .map_err(|e| format!("{e:?}"))?;
        let latency = t0.elapsed();
        let v = validate(
            &bytes,
            std::slice::from_ref(q),
            &self.model,
            &GateThresholds::default(),
        )
        .map_err(|k| format!("malformed: {}", k.slug()))?;
        Ok(Asked {
            probs: choice_probs(&bytes, q),
            result: v.results[0].1,
            input_tokens: v.input_tokens,
            latency,
        })
    }

    async fn health(&self) -> Result<bool, String> {
        let deadline = Duration::from_secs(2);
        let slug = super::startup::health(&self.ep, &self.model, deadline).await;
        let chat = post_json(
            &self.ep,
            "/v1/chat/completions",
            r#"{"model":"x","messages":[{"role":"user","content":"hi"}]}"#,
            deadline,
        )
        .await;
        let fingerprint = matches!(chat, Err(PostError::Transport(TransportErr::Http(501))));
        let ok = slug.is_ok() && fingerprint;
        self.report(
            "health",
            ok,
            &[
                ("endpoint", self.ep.to_string()),
                ("model", self.model.clone()),
                (
                    "systemone",
                    slug.err().unwrap_or_else(|| "ok, slug matches".into()),
                ),
                ("chat_501_fingerprint", format!("{fingerprint} ({chat:?})")),
            ],
        );
        Ok(ok)
    }

    async fn ask_file(&self, path: &Path, q: ProbeQuestion) -> Result<bool, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let state = raw_state(text.trim_end());
        let a = self.ask("ask", &state, &q.spec()).await?;
        let st = stats(&a.probs);
        self.report(
            "ask",
            true,
            &[
                ("probabilities", format!("{:?}", a.probs)),
                ("result", format!("{:?}", a.result)),
                (
                    "margin",
                    st.map_or("-".into(), |s| format!("{:.4}", s.margin)),
                ),
                (
                    "n_eff",
                    st.map_or("-".into(), |s| format!("{:.3}", s.n_eff)),
                ),
                ("input_tokens", format!("{:?}", a.input_tokens)),
                ("latency_ms", a.latency.as_millis().to_string()),
            ],
        );
        Ok(true)
    }

    async fn sensitivity(&self) -> Result<bool, String> {
        let pairs = [
            (
                "posture: strong vs weak",
                ProbeQuestion::Posture,
                scenario(true, false),
                scenario(false, false),
            ),
            (
                "objective: armor near vs far",
                ProbeQuestion::Objective,
                scenario(false, true),
                scenario(false, false),
            ),
        ];
        let mut all = true;
        let mut lines = Vec::new();
        for (name, q, a, b) in pairs {
            let (pa, pb) = (
                self.probs_of("sensitivity", &a, q).await?,
                self.probs_of("sensitivity", &b, q).await?,
            );
            let d = l1(&pa, &pb);
            let moved = d >= MOVED_L1;
            all &= moved;
            lines.push((name, format!("moved={moved} L1={d:.4} a={pa:?} b={pb:?}")));
        }
        let fields: Vec<(&str, String)> = lines.into_iter().collect();
        self.report("sensitivity", all, &fields);
        Ok(all)
    }

    async fn anticorrelation(&self) -> Result<bool, String> {
        let q = ProbeQuestion::Objective;
        let a = self
            .probs_of("anticorrelation", &scenario(true, false), q)
            .await?;
        let b = self
            .probs_of("anticorrelation", &scenario(false, true), q)
            .await?;
        let ok = a != b;
        self.report(
            "anticorrelation",
            ok,
            &[
                ("different", ok.to_string()),
                ("a", format!("{a:?}")),
                ("b", format!("{b:?}")),
            ],
        );
        Ok(ok)
    }

    async fn flatness(&self) -> Result<bool, String> {
        let state = render_state(&scenario(true, false), usize::MAX);
        let mut all = true;
        let mut fields = Vec::new();
        for n in [3usize, 24] {
            let q = meaningless(n);
            let a = self.ask("flatness", &state, &q).await?;
            let rejected = matches!(a.result, QuestionResult::Choice(Decision::NoSignal { .. }));
            all &= rejected;
            let top = a.probs.iter().cloned().fold(0.0f32, f32::max);
            fields.push((
                if n == 3 { "3_options" } else { "24_options" },
                format!("rejected={rejected} top_p={top:.4}"),
            ));
        }
        self.report("flatness", all, &fields);
        Ok(all)
    }

    async fn probs_of(
        &self,
        sub: &str,
        s: &DecisionState,
        q: ProbeQuestion,
    ) -> Result<Vec<f32>, String> {
        Ok(self
            .ask(sub, &render_state(s, usize::MAX), &q.spec())
            .await?
            .probs)
    }

    /// Print a result block (or one JSON line with `--json`).
    fn report(&self, name: &str, ok: bool, fields: &[(&str, String)]) {
        if self.json {
            let mut m = serde_json::Map::new();
            m.insert("check".into(), name.into());
            m.insert("ok".into(), ok.into());
            for (k, v) in fields {
                m.insert((*k).into(), v.clone().into());
            }
            println!("{}", serde_json::Value::Object(m));
        } else {
            println!("jev-probe {name}: {}", if ok { "PASS" } else { "FAIL" });
            for (k, v) in fields {
                println!("  {k:<22} {v}");
            }
        }
    }
}

impl Probe {
    async fn calibrate(&self, dir: &Path) -> Result<bool, String> {
        let th = GateThresholds::default();
        let mut all = true;
        let mut fields = Vec::new();
        for q in [ProbeQuestion::Posture, ProbeQuestion::Objective] {
            let qdir = dir.join(q.dir_name());
            if !qdir.exists() {
                continue;
            }
            // Plan 73 T4: calibrate the PRODUCTION wording on the HOLDOUT half only (the lab chose
            // the wording on the tuning half). Only acting answers count as ambiguous/flat leakage:
            // a confident no-op ("hold" / "carry_on") on an unclear state changes nothing.
            use super::lab::{half_of, load_corpus, noop_label, CaseKind, Half, PRODUCTION};
            let base = q.spec();
            let spec = PRODUCTION.apply("", &base).1;
            let noop = noop_label(&spec);
            let cases: Vec<_> = load_corpus(&qdir, &base)?
                .into_iter()
                .filter(|c| half_of(&c.name) == Half::Holdout)
                .collect();
            let count = |f: fn(&CaseKind) -> bool| cases.iter().filter(|c| f(&c.kind)).count();
            let (n_ob, n_am) = (
                count(|k| matches!(k, CaseKind::Obvious(_))),
                count(|k| *k == CaseKind::Ambiguous),
            );
            if n_ob < 10 || n_am < 10 {
                return Err(format!(
                    "{}: the holdout needs ≥10 obvious and ≥10 ambiguous (has {n_ob} / {n_am})",
                    qdir.display()
                ));
            }
            let mut obvious = Vec::new();
            let mut margins = [Vec::new(), Vec::new()];
            for c in &cases {
                let text = PRODUCTION.apply(&c.text, &base).0;
                let a = self.ask("calibrate", &raw_state(&text), &spec).await?;
                let st = stats(&a.probs);
                let top = st.and_then(|st| option_label(&spec, st.top));
                let margin = st.map_or(0.0, |st| st.margin);
                let acting_margin = if top.is_some() && top == noop {
                    0.0
                } else {
                    margin
                };
                match c.kind {
                    CaseKind::Obvious(expected) => obvious.push(ObviousAnswer {
                        margin,
                        correct: top == Some(expected),
                    }),
                    CaseKind::Ambiguous => margins[0].push(acting_margin),
                    CaseKind::Flat => margins[1].push(acting_margin),
                }
            }
            let wrong = obvious.iter().filter(|o| !o.correct).count();
            let t = calibrate_threshold(&obvious, &margins[0], &margins[1]);
            all &= t.is_some();
            let current = if q == ProbeQuestion::Posture {
                th.margin_binary
            } else {
                th.margin_multi
            };
            fields.push((
                if q == ProbeQuestion::Posture { "posture (binary)" } else { "objective (3-way)" },
                match t {
                    Some(t) => format!(
                        "threshold={t:.3} (provisional {current:.2}); obvious picked wrong: {wrong}/{}",
                        obvious.len()
                    ),
                    None => format!(
                        "NO threshold applies ≥80% of obvious correctly, ≤5% of obvious wrongly, \
                         ≤10% ambiguous, 0% flat — delete the question (obvious picked wrong: \
                         {wrong}/{})",
                        obvious.len()
                    ),
                },
            ));
        }
        if fields.is_empty() {
            return Err(format!(
                "no labelled sets under {} (see `jev-probe calibrate --help`)",
                dir.display()
            ));
        }
        self.report("calibrate", all, &fields);
        Ok(all)
    }

    async fn bench(
        &self,
        rate: f64,
        conc: usize,
        dur: Duration,
        deadline: Duration,
    ) -> Result<bool, String> {
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(conc));
        let q = ProbeQuestion::Posture.spec();
        let states = [
            render_state(&scenario(true, false), usize::MAX),
            render_state(&scenario(false, true), usize::MAX),
        ];
        let interval = Duration::from_secs_f64(1.0 / rate.max(0.01));
        let t0 = Instant::now();
        let mut tasks = Vec::new();
        let mut i = 0usize;
        while t0.elapsed() < dur {
            let body = render_request(&states[i % 2], std::slice::from_ref(&q));
            let (sem, ep) = (sem.clone(), self.ep.clone());
            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire_owned().await.ok()?;
                let t = Instant::now();
                let r = post_json(&ep, "/v1/systemone", &body, deadline).await;
                Some((t.elapsed(), r.map(|b| input_tokens(&b))))
            }));
            i += 1;
            tokio::time::sleep(interval).await;
        }
        let mut lat = Vec::new();
        let (mut tokens, mut errors) = (0u64, 0usize);
        for t in tasks {
            match t.await.ok().flatten() {
                Some((d, Ok(tok))) => {
                    lat.push(d.as_secs_f64() * 1000.0);
                    tokens += u64::from(tok.unwrap_or(0));
                }
                _ => errors += 1,
            }
        }
        let secs = t0.elapsed().as_secs_f64();
        let tok_s = tokens as f64 / secs;
        let p95 = percentile(&mut lat, 95.0);
        let fits = p95 <= deadline.as_millis() as f64;
        self.report("bench", fits && errors == 0, &[
            ("requests", format!("{} ok, {errors} failed, target {rate:.2} req/s, concurrency {conc}", lat.len())),
            ("latency_ms", format!("p50={:.0} p95={p95:.0} p99={:.0}", percentile(&mut lat, 50.0), percentile(&mut lat, 99.0))),
            ("input_tok_s", format!("{tok_s:.0} = {:.0}% of single-stream {SINGLE_STREAM_TOK_S:.0}, {:.0}% of hammered {HAMMERED_TOK_S:.0}", 100.0 * tok_s / SINGLE_STREAM_TOK_S, 100.0 * tok_s / HAMMERED_TOK_S)),
            ("p95_vs_deadline", format!("{} (deadline {} ms) — if not, raise dwell before raising the deadline", if fits { "fits" } else { "EXCEEDS" }, deadline.as_millis())),
        ]);
        Ok(fits && errors == 0)
    }

    async fn replay(&self, file: &Path) -> Result<bool, String> {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let (mut same, mut changed, mut failed) = (0usize, 0usize, 0usize);
        let mut worst = 0.0f32;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let rec: serde_json::Value =
                serde_json::from_str(line).map_err(|e| format!("bad harvest line: {e}"))?;
            let (Some(req), Some(then)) = (rec["request"].as_str(), rec["response"].as_str())
            else {
                continue;
            };
            match post_json(&self.ep, "/v1/systemone", req, PROBE_DEADLINE).await {
                Ok(now) => {
                    let d = max_prob_diff(then.as_bytes(), &now);
                    worst = worst.max(d);
                    if d == 0.0 {
                        same += 1
                    } else {
                        changed += 1
                    }
                }
                Err(_) => failed += 1,
            }
        }
        let ok = changed == 0 && failed == 0;
        self.report(
            "replay",
            ok,
            &[
                ("identical", same.to_string()),
                (
                    "changed",
                    format!(
                        "{changed} (max |Δp| {worst:.4}) — a server/model change moved the readout"
                    ),
                ),
                ("failed", failed.to_string()),
            ],
        );
        Ok(ok)
    }
}

impl Probe {
    /// Score each variant on one corpus half, per question: one result row per variant.
    async fn lab(
        &self,
        dir: &Path,
        qs: &[ProbeQuestion],
        vs: &[super::lab::Variant],
        half: super::lab::Half,
        detail: Option<&Path>,
    ) -> Result<bool, String> {
        use super::lab::{half_of, load_corpus, score, CaseKind};
        let mut detail_lines = Vec::new();
        for &q in qs {
            let base = q.spec();
            let cases: Vec<_> = load_corpus(&dir.join(q.dir_name()), &base)?
                .into_iter()
                .filter(|c| half_of(&c.name) == half)
                .collect();
            if cases.is_empty() {
                return Err(format!(
                    "no {half:?} cases for {:?} under {}",
                    q,
                    dir.display()
                ));
            }
            let t = if q == ProbeQuestion::Posture {
                self.gate.margin_binary
            } else {
                self.gate.margin_multi
            };
            for &v in vs {
                let mut scored = Vec::new();
                let mut errors = 0usize;
                for c in &cases {
                    let (text, spec) = v.apply(&c.text, &base);
                    match self.ask("lab", &raw_state(&text), &spec).await {
                        Ok(a) => {
                            let st = stats(&a.probs);
                            let top = st.and_then(|st| option_label(&spec, st.top)).unwrap_or("?");
                            let margin = st.map_or(0.0, |st| st.margin);
                            let (kind, expected) = match c.kind {
                                CaseKind::Obvious(l) => ("obvious", Some(l)),
                                CaseKind::Ambiguous => ("ambiguous", None),
                                CaseKind::Flat => ("flat", None),
                            };
                            detail_lines.push(
                                serde_json::json!({
                                    "question": q.dir_name(), "variant": format!("{v:?}"),
                                    "half": format!("{half:?}"), "case": c.name, "kind": kind,
                                    "expected": expected, "top": top, "margin": margin,
                                    "probabilities": a.probs,
                                })
                                .to_string(),
                            );
                            scored.push(super::lab::Scored {
                                kind: c.kind.clone(),
                                top,
                                margin,
                            });
                        }
                        Err(_) => errors += 1,
                    }
                }
                let m = score(&scored, t, super::lab::noop_label(&v.apply("", &base).1));
                let obvious = cases
                    .iter()
                    .filter(|c| matches!(c.kind, CaseKind::Obvious(_)))
                    .count();
                self.report(
                    &format!("lab {} {v:?} ({half:?})", q.dir_name()),
                    errors == 0,
                    &[
                        (
                            "cases",
                            format!("{} ({obvious} obvious), {errors} failed", cases.len()),
                        ),
                        ("threshold", format!("{t:.2}")),
                        ("right_confident", pct(m.right_confident)),
                        ("confident_wrong", pct(m.confident_wrong)),
                        ("accuracy", pct(m.accuracy)),
                        ("margin_correct", format!("{:.3}", m.margin_correct)),
                        ("ambiguous_leak", pct(m.ambiguous_leak)),
                        ("ambiguous_leak_acting", pct(m.ambiguous_leak_acting)),
                        ("flat_max", format!("{:.3}", m.flat_max)),
                        ("flat_max_acting", format!("{:.3}", m.flat_max_acting)),
                        ("separation", format!("{:.3}", m.separation)),
                    ],
                );
            }
        }
        if let Some(path) = detail {
            std::fs::write(path, detail_lines.join("\n") + "\n")
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(true)
    }
}

/// `jev-probe journal`: print the arena report; optionally write candidate scenarios.
fn journal_report(
    files: &[PathBuf],
    candidates: Option<&Path>,
    limit: usize,
) -> Result<bool, String> {
    use super::journal::{candidates as worst, parse_line, report};
    let mut entries = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
        entries.extend(text.lines().filter_map(parse_line));
    }
    print!("{}", report(&entries));
    if let Some(dir) = candidates {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let c = worst(&entries, limit);
        for (stem, state) in &c {
            let path = dir.join(format!("{stem}.txt"));
            std::fs::write(&path, format!("{state}\n"))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        println!(
            "{} candidate scenarios written to {} — label them before use",
            c.len(),
            dir.display()
        );
    }
    Ok(true)
}

fn pct(x: f32) -> String {
    format!("{:.0}%", 100.0 * x)
}

/// Print the tuning/holdout counts per question — no model calls (T1's split check).
fn lab_split_report(dir: &Path, qs: &[ProbeQuestion]) -> Result<bool, String> {
    use super::lab::{load_corpus, split_counts, Half};
    let mut ok = true;
    for &q in qs {
        let cases = load_corpus(&dir.join(q.dir_name()), &q.spec())?;
        for half in [Half::Tuning, Half::Holdout] {
            let (o, a, f) = split_counts(&cases, half);
            let enough = o >= 10 && a >= 10;
            ok &= enough;
            println!(
                "{:<9} {half:?}: {o} obvious, {a} ambiguous, {f} flat{}",
                q.dir_name(),
                if enough {
                    ""
                } else {
                    "  (need ≥10 obvious and ≥10 ambiguous)"
                }
            );
        }
    }
    Ok(ok)
}

/// A rendered state from operator-authored text (calibration sets, `ask --state`). These files
/// are written by the human running the probe, not taken from the wire.
fn raw_state(text: &str) -> RenderedState {
    RenderedState {
        text: text.to_string(),
        dropped: Vec::new(),
        est_tokens: est_tokens(text),
    }
}

/// A `choice` question over `n` options that say nothing (the flatness probe).
fn meaningless(n: usize) -> QuestionSpec {
    const LABELS: [&str; 24] = [
        "opt_a", "opt_b", "opt_c", "opt_d", "opt_e", "opt_f", "opt_g", "opt_h", "opt_i", "opt_j",
        "opt_k", "opt_l", "opt_m", "opt_n", "opt_o", "opt_p", "opt_q", "opt_r", "opt_s", "opt_t",
        "opt_u", "opt_v", "opt_w", "opt_x",
    ];
    QuestionSpec {
        key: QuestionKey::Objective,
        instructions: "Choose one option.",
        kind: QuestionKind::Choice(
            LABELS[..n.min(24)]
                .iter()
                .map(|&label| ChoiceOption {
                    label,
                    desc: Phrase::lit("an option"),
                })
                .collect(),
        ),
    }
}

/// One obvious calibration case: the answer's margin, and whether it picked the expected label.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObviousAnswer {
    /// `p1 − p2`.
    pub margin: f32,
    /// Did the top choice match the pre-registered expected label?
    pub correct: bool,
}

/// The lowest margin threshold `t` at which the gate would act on
/// - **≥ 80 %** of the obvious cases **with the right answer** (`correct && margin ≥ t`),
/// - **≤ 5 %** of the obvious cases **with the wrong answer** (acting confidently wrong on a case
///   any player gets right is disqualifying),
/// - **≤ 10 %** of the ambiguous cases, and **none** of the flat ones.
///
/// `None` if no threshold satisfies all four — the question is then deleted, not tuned.
pub fn calibrate_threshold(
    obvious: &[ObviousAnswer],
    ambiguous: &[f32],
    flat: &[f32],
) -> Option<f32> {
    let share = |n: usize, of: usize| if of == 0 { 0.0 } else { n as f32 / of as f32 };
    let mut candidates: Vec<f32> = obvious
        .iter()
        .map(|o| o.margin)
        .chain(ambiguous.iter().copied())
        .chain(flat.iter().copied())
        .collect();
    candidates.push(1.0);
    candidates.sort_by(f32::total_cmp);
    candidates.into_iter().find(|&t| {
        let right = obvious
            .iter()
            .filter(|o| o.correct && o.margin >= t)
            .count();
        let wrong = obvious
            .iter()
            .filter(|o| !o.correct && o.margin >= t)
            .count();
        let amb = ambiguous.iter().filter(|&&m| m >= t).count();
        share(right, obvious.len()) >= 0.8
            && share(wrong, obvious.len()) <= 0.05
            && share(amb, ambiguous.len()) <= 0.1
            && flat.iter().all(|&m| m < t)
    })
}

/// The label of option `i` of a choice question.
fn option_label(q: &QuestionSpec, i: usize) -> Option<&'static str> {
    match &q.kind {
        QuestionKind::Choice(opts) => opts.get(i).map(|o| o.label),
        _ => None,
    }
}

/// The `p`th percentile (nearest rank) of `xs`; 0 when empty.
pub fn percentile(xs: &mut [f64], p: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * xs.len() as f64).ceil().max(1.0) as usize;
    xs[rank.min(xs.len()) - 1]
}

fn l1(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum()
}

/// The first answer's probabilities in offered order (for display and distance).
fn choice_probs(body: &[u8], q: &QuestionSpec) -> Vec<f32> {
    let v: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let p = &v["answers"][q.key.wire_key()]["probabilities"];
    match &q.kind {
        QuestionKind::Choice(opts) => opts
            .iter()
            .map(|o| p[o.label].as_f64().unwrap_or(0.0) as f32)
            .collect(),
        _ => Vec::new(),
    }
}

fn input_tokens(body: &[u8]) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v["usage"]["input_tokens"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
}

/// Largest absolute probability difference across every answer of two response bodies.
fn max_prob_diff(then: &[u8], now: &[u8]) -> f32 {
    let parse = |b: &[u8]| serde_json::from_slice::<serde_json::Value>(b).unwrap_or_default();
    let (a, b) = (parse(then), parse(now));
    let mut worst = 0.0f64;
    if let Some(answers) = a["answers"].as_object() {
        for (key, ans) in answers {
            if let Some(ps) = ans["probabilities"].as_object() {
                for (label, p) in ps {
                    let q = b["answers"][key]["probabilities"][label]
                        .as_f64()
                        .unwrap_or(-1.0);
                    worst = worst.max((p.as_f64().unwrap_or(0.0) - q).abs());
                }
            }
        }
    }
    worst as f32
}

/// Append one request/response pair to [`HARVEST`] (best effort — a probe never fails on it).
fn harvest(sub: &str, request: &str, r: &Result<Vec<u8>, PostError>) {
    let response = match r {
        Ok(b) => serde_json::Value::String(String::from_utf8_lossy(b).into_owned()),
        Err(_) => serde_json::Value::Null,
    };
    let err = r.as_ref().err().map(|e| format!("{e:?}"));
    let line = serde_json::json!({
        "ts": time::OffsetDateTime::now_utc().unix_timestamp(),
        "sub": sub,
        "request": request,
        "response": response,
        "error": err,
    });
    let path = Path::new(HARVEST);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jev_probe_default_rate_is_the_refresh_demand() {
        assert!((default_rate(36, 15.0) - 2.4).abs() < 1e-9);
        assert!(
            (default_rate(36, 20.0) - 1.8).abs() < 1e-9,
            "the W5 re-price"
        );
    }

    fn right(margins: &[f32]) -> Vec<ObviousAnswer> {
        margins
            .iter()
            .map(|&margin| ObviousAnswer {
                margin,
                correct: true,
            })
            .collect()
    }

    #[test]
    fn jev_probe_calibration_picks_the_lowest_separating_margin() {
        let obvious = right(&[0.9, 0.85, 0.8, 0.76, 0.7, 0.66, 0.6, 0.55, 0.5, 0.2]);
        let ambiguous = [0.05, 0.1, 0.12, 0.2, 0.25, 0.3, 0.33, 0.35, 0.4, 0.45];
        let flat = [0.01, 0.02];
        // ≥80 % of obvious needs t ≤ 0.55; ≤10 % of ambiguous needs t > 0.40.
        assert_eq!(calibrate_threshold(&obvious, &ambiguous, &flat), Some(0.45));
    }

    #[test]
    fn jev_probe_calibration_confident_wrong_answers_disqualify() {
        // Same margins, but the two most confident "obvious" answers picked the WRONG label:
        // only 7/10 are right-and-confident at any useful t, and 2/10 would be acted on wrongly.
        let mut obvious = right(&[0.9, 0.85, 0.8, 0.76, 0.7, 0.66, 0.6, 0.55, 0.5, 0.2]);
        obvious[0].correct = false;
        obvious[1].correct = false;
        let ambiguous = [0.05, 0.1, 0.12, 0.2, 0.25, 0.3, 0.33, 0.35, 0.4, 0.45];
        assert_eq!(calibrate_threshold(&obvious, &ambiguous, &[]), None);
        // A wrong answer that is NOT confident is harmless: the gate would never act on it.
        let mut quiet = right(&[0.9, 0.85, 0.8, 0.76, 0.7, 0.66, 0.6, 0.55, 0.5, 0.2]);
        quiet[9].correct = false;
        assert_eq!(calibrate_threshold(&quiet, &ambiguous, &[]), Some(0.45));
    }

    #[test]
    fn jev_probe_calibration_says_no_when_sets_overlap() {
        assert_eq!(
            calibrate_threshold(&right(&[0.3; 10]), &[0.3; 10], &[]),
            None
        );
        // A flat answer above every obvious one forbids every threshold below it.
        assert_eq!(
            calibrate_threshold(&right(&[0.6; 10]), &[0.0; 10], &[0.9]),
            None
        );
    }

    #[test]
    fn jev_probe_percentile_nearest_rank() {
        let mut xs = vec![5.0, 1.0, 4.0, 2.0, 3.0];
        assert_eq!(percentile(&mut xs, 50.0), 3.0);
        assert_eq!(percentile(&mut xs, 95.0), 5.0);
        assert_eq!(percentile(&mut [], 50.0), 0.0);
    }

    #[test]
    fn jev_probe_catalogue_renders_through_the_production_renderer() {
        let body = render_request(
            &render_state(&scenario(false, true), usize::MAX),
            &[ProbeQuestion::Objective.spec()],
        );
        let v: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        assert_eq!(v["questions"]["objective"]["type"], "choice");
        assert!(v["state"]
            .as_str()
            .is_some_and(|s| s.contains("armor:touch")));
        assert_eq!(meaningless(24).key, QuestionKey::Objective);
    }

    #[test]
    fn jev_probe_diff_and_parse_helpers() {
        let a = br#"{"answers":{"posture":{"probabilities":{"x":0.7,"y":0.3}}},"usage":{"input_tokens":9}}"#;
        let b = br#"{"answers":{"posture":{"probabilities":{"x":0.6,"y":0.4}}}}"#;
        assert!((max_prob_diff(a, b) - 0.1).abs() < 1e-6);
        assert_eq!(max_prob_diff(a, a), 0.0);
        assert_eq!(input_tokens(a), Some(9));
        assert!((l1(&[0.7, 0.3], &[0.6, 0.4]) - 0.2).abs() < 1e-6);
    }
}
