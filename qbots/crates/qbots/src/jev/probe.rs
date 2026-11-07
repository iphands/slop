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
    /// `<dir>/<question>/{obvious,ambiguous,flat}/*.txt` (≥10 obvious, ≥10 ambiguous).
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
}

/// The probe's question catalogue (the brain's real objective/posture questions arrive with
/// T10/T12; the probe will ask those once they exist).
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ProbeQuestion {
    /// Binary posture: disengage / hold.
    Posture,
    /// Three-way objective: seek health / seek armor / hold position.
    Objective,
}

impl ProbeQuestion {
    fn spec(self) -> QuestionSpec {
        let opt = |label, desc| ChoiceOption {
            label,
            desc: Phrase::lit(desc),
        };
        match self {
            ProbeQuestion::Posture => QuestionSpec {
                key: QuestionKey::Posture,
                instructions: "Should this bot break off the current fight to survive, or leave \
                               the fight to its own judgement?",
                kind: QuestionKind::Choice(vec![
                    opt("disengage", "retreat now and break line of sight"),
                    opt("hold", "keep fighting as the bot sees fit"),
                ]),
            },
            ProbeQuestion::Objective => QuestionSpec {
                key: QuestionKey::Objective,
                instructions: "Choose this bot's next objective.",
                kind: QuestionKind::Choice(vec![
                    opt("seek_health", "go to the nearest health pickup"),
                    opt("seek_armor", "go to the nearest armor pickup"),
                    opt("hold_position", "stay and control the current area"),
                ]),
            },
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
            let qdir = dir.join(
                q.to_possible_value()
                    .map_or("?".into(), |v| v.get_name().to_string()),
            );
            if !qdir.exists() {
                continue;
            }
            let set = |label: &str| read_states(&qdir.join(label));
            let (ob, am, fl) = (set("obvious")?, set("ambiguous")?, set("flat")?);
            if ob.len() < 10 || am.len() < 10 {
                return Err(format!(
                    "{}: need ≥10 obvious and ≥10 ambiguous pre-registered states (have {} / {})",
                    qdir.display(),
                    ob.len(),
                    am.len()
                ));
            }
            let mut margins = [Vec::new(), Vec::new(), Vec::new()];
            for (i, states) in [ob, am, fl].iter().enumerate() {
                for s in states {
                    let a = self.ask("calibrate", &raw_state(s), &q.spec()).await?;
                    margins[i].push(stats(&a.probs).map_or(0.0, |st| st.margin));
                }
            }
            let t = calibrate_threshold(&margins[0], &margins[1], &margins[2]);
            all &= t.is_some();
            let current = if q == ProbeQuestion::Posture {
                th.margin_binary
            } else {
                th.margin_multi
            };
            fields.push((
                if q == ProbeQuestion::Posture { "posture (binary)" } else { "objective (3-way)" },
                match t {
                    Some(t) => format!("threshold={t:.3} (provisional {current:.2})"),
                    None => "NO threshold applies ≥80% obvious, ≤10% ambiguous, 0% flat — delete the question".into(),
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

/// A rendered state from operator-authored text (calibration sets, `ask --state`). These files
/// are written by the human running the probe, not taken from the wire.
fn raw_state(text: &str) -> RenderedState {
    RenderedState {
        text: text.to_string(),
        dropped: Vec::new(),
        est_tokens: est_tokens(text),
    }
}

/// Every `*.txt` under `dir`, sorted by file name (missing dir = empty set).
fn read_states(dir: &Path) -> Result<Vec<String>, String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| {
            std::fs::read_to_string(p)
                .map(|s| s.trim_end().to_string())
                .map_err(|e| format!("{}: {e}", p.display()))
        })
        .collect()
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

/// The lowest margin threshold that applies ≥ 80 % of `obvious`, ≤ 10 % of `ambiguous` and
/// none of `flat`; `None` if no threshold satisfies all three (the question is then deleted).
pub fn calibrate_threshold(obvious: &[f32], ambiguous: &[f32], flat: &[f32]) -> Option<f32> {
    let rate = |xs: &[f32], t: f32| {
        if xs.is_empty() {
            0.0
        } else {
            xs.iter().filter(|&&m| m >= t).count() as f32 / xs.len() as f32
        }
    };
    let mut candidates: Vec<f32> = obvious
        .iter()
        .chain(ambiguous)
        .chain(flat)
        .copied()
        .collect();
    candidates.push(1.0);
    candidates.sort_by(f32::total_cmp);
    candidates.into_iter().find(|&t| {
        rate(obvious, t) >= 0.8 && rate(ambiguous, t) <= 0.1 && flat.iter().all(|&m| m < t)
    })
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

    #[test]
    fn jev_probe_calibration_picks_the_lowest_separating_margin() {
        let obvious = [0.9, 0.85, 0.8, 0.76, 0.7, 0.66, 0.6, 0.55, 0.5, 0.2];
        let ambiguous = [0.05, 0.1, 0.12, 0.2, 0.25, 0.3, 0.33, 0.35, 0.4, 0.45];
        let flat = [0.01, 0.02];
        // ≥80 % of obvious needs t ≤ 0.55; ≤10 % of ambiguous needs t > 0.40.
        assert_eq!(calibrate_threshold(&obvious, &ambiguous, &flat), Some(0.45));
    }

    #[test]
    fn jev_probe_calibration_says_no_when_sets_overlap() {
        let obvious = [0.3; 10];
        let ambiguous = [0.3; 10];
        assert_eq!(calibrate_threshold(&obvious, &ambiguous, &[]), None);
        // A flat answer above every obvious one forbids every threshold below it.
        assert_eq!(calibrate_threshold(&[0.6; 10], &[0.0; 10], &[0.9]), None);
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
