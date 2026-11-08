//! # qbots::jev::hub — the process-wide decision hub (Plan 72 T5)
//!
//! The **only** place that talks to the decision model. Bots never wait on it: a bot's brain
//! `submit`s an ask (a short lock + a notify) and reads `latest()` (a lock + a `Copy`), both
//! synchronous, both from inside `Brain::tick`.
//!
//! - **Slots, per bot:** a newest-wins *pending* ask (never a queue — a queue spends the budget
//!   on the past) and the latest gated *decision* (`Copy`, read as `None` past `ttl`).
//! - **Workers — one pull model:** exactly `max_concurrent` tasks loop pick → admit → send →
//!   classify → gate → store. *Pick* drops pendings older than `snapshot_ttl`
//!   (`skipped_no_capacity`), then takes the **oldest event**, else the **oldest refresh** while
//!   fewer than `refresh_max_inflight` refreshes are in flight. No permit wait, so a picked ask is
//!   at most `snapshot_ttl` old when its send starts; oldest-first means no bot starves.
//! - **Admission:** the fleet token [`Bucket`] (reserve `chars/4`, true up with
//!   `usage.input_tokens`).
//! - **Pacing, per bot and per question key (at `submit`; Plan 74 T4 split it by key so a
//!   roaming objective ask never paces out the contact posture ask after it):** `event_floor`
//!   between any two asks of one key, refresh every
//!   `min_dwell`, both jittered one-sided ×[1, `jitter_max`]; an ask resets the refresh timer.
//! - **Generation, per bot:** bumped on a level change; an answer carrying an older generation
//!   is discarded before it is stored.
//! - **Breaker, tiered:** `breaker_hard` consecutive refused / HTTP-error / closed / model
//!   mismatches, **or** `breaker_timeouts` consecutive timeouts, open it for `breaker_open`;
//!   while open, picks drop asks (`circuit`). One `error!` per opening, naming the cause.
//! - All clocks are `tokio::time::Instant` (the hub's clock; the brain keeps its own).
//!
//! The optional response cache is **cut** (plan cut order #1): off by default, a herding hazard,
//! and no consumer until T15 decides it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use brain::jev::decision::{Decision, MalformedKind, TransportErr};
use brain::jev::gate::GateThresholds;
use brain::jev::question::{QuestionKey, QuestionKind};
use brain::jev::source::{AppliedDecision, ApplyOutcome, Ask, DecisionSource, TicketId};
use brain::jev::state::{Field, FieldKey, MapId};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::bucket::Bucket;
use super::net::PostError;
use super::render::{est_tokens, render_request, render_state};
use super::transport::DecisionTransport;
use super::wire::{validate, QuestionResult, Validated};

/// Hub tunables. `Default` = the plan's budget table (W0–W4 values).
#[derive(Clone, Debug)]
pub struct HubConfig {
    /// The model slug every response must report.
    pub model: String,
    /// Worker count.
    pub max_concurrent: usize,
    /// Refresh asks in flight at most (events may use every worker).
    pub refresh_max_inflight: usize,
    /// The fleet input-token budget.
    pub max_input_tokens_per_s: f64,
    /// Hard cap on the rendered state's estimated tokens.
    pub max_state_tokens: usize,
    /// A pending ask older than this is dropped, never sent.
    pub snapshot_ttl: Duration,
    /// A stored decision older than this reads as `None`.
    pub ttl: Duration,
    /// Background refresh period per bot (before jitter).
    pub min_dwell: Duration,
    /// Minimum interval between any two asks of one bot (before jitter).
    pub event_floor: Duration,
    /// One-sided jitter: each interval is multiplied by a factor in `[1, jitter_max]`.
    pub jitter_max: f64,
    /// Consecutive non-timeout failures that open the breaker.
    pub breaker_hard: u32,
    /// Consecutive timeouts that open the breaker.
    pub breaker_timeouts: u32,
    /// How long the breaker stays open.
    pub breaker_open: Duration,
    /// The gate.
    pub gate: GateThresholds,
    /// Plan 73 decision journal: append one JSON line per decision, with its outcome, here.
    pub journal: Option<std::path::PathBuf>,
    /// Plan 73 shadow mode: `jev_static` bots ask and journal too, never steering.
    pub shadow: bool,
    /// Plan 73 arena round A2: a `disengage` only counts when the asking bot is hurt (health band
    /// `crit`/`lo`); on a healthy bot it is suppressed (stored as no posture, journalled
    /// `suppressed`).
    pub disengage_needs_hurt: bool,
    /// Plan 74 B9: `false` suppresses every posture answer (journalled `suppressed`).
    pub posture_live: bool,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            model: "h2oai/h2o-lightning-4b".into(),
            max_concurrent: 3,
            refresh_max_inflight: 2,
            max_input_tokens_per_s: 1000.0,
            max_state_tokens: 512,
            snapshot_ttl: Duration::from_millis(1500),
            ttl: Duration::from_secs(24),
            min_dwell: Duration::from_secs(15),
            event_floor: Duration::from_secs(5),
            jitter_max: 1.3,
            breaker_hard: 3,
            breaker_timeouts: 8,
            breaker_open: Duration::from_secs(5),
            gate: GateThresholds::default(),
            journal: None,
            shadow: false,
            disengage_needs_hurt: false,
            posture_live: false,
        }
    }
}

/// Hub counters (T11 turns these into the report and `EVT jev_fleet`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    /// Asks accepted by `submit`.
    pub asked: u64,
    /// Asks refused by per-bot pacing.
    pub paced: u64,
    /// Asks a worker picked, admitted and sent.
    pub admitted: u64,
    /// Pending asks that aged past `snapshot_ttl` before a worker was free.
    pub skipped_no_capacity: u64,
    /// Asks refused by the token bucket.
    pub budget: u64,
    /// Asks dropped while the breaker was open.
    pub circuit: u64,
    /// Requests that got no usable response.
    pub transport: u64,
    /// Responses that broke the contract.
    pub malformed: u64,
    /// Answers discarded because the bot's generation moved on.
    pub stale_generation: u64,
    /// Answers stored in a slot.
    pub stored: u64,
    /// `ack(Applied)` — the headline numerator.
    pub applied: u64,
    /// `ack(Rejected(_))`.
    pub rejected: u64,
    /// Times the breaker opened.
    pub breaker_opened: u64,
    /// Stored answers where at least one question passed the gate.
    pub decisive: u64,
    /// Stored answers where every question was gated off (`NoSignal`).
    pub no_signal: u64,
    /// Asks from shadow bots (Plan 73) — kept out of `asked` so the headline is jev's own.
    pub shadow_asked: u64,
    /// Decisions resolved by shadow bots (never applied).
    pub shadowed: u64,
}

impl Counters {
    /// `applied / asked` — the headline (everything we wanted to ask and could not counts).
    pub fn applied_rate(&self) -> f64 {
        ratio(self.applied, self.asked)
    }

    /// `no_signal / stored` — when an answer came back, how often it did not discriminate.
    pub fn gate_reject_rate(&self) -> f64 {
        ratio(self.no_signal, self.stored)
    }

    /// `(asked − admitted) / asked` — was the budget/worker capacity the limiter?
    pub fn admission_drop_rate(&self) -> f64 {
        ratio(self.asked.saturating_sub(self.admitted), self.asked)
    }
}

fn ratio(n: u64, d: u64) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 / d as f64
    }
}

struct Pending {
    ticket: TicketId,
    ask: Ask,
    created: Instant,
    generation: u64,
}

#[derive(Default)]
struct Pacing {
    last_ask: Option<Instant>,
    floor: Duration,
    dwell: Duration,
}

struct BotSlot {
    pending: Mutex<Option<Pending>>,
    decision: Mutex<Option<(AppliedDecision, Instant)>>,
    /// One pacing clock per question key ([`pace_idx`]).
    pacing: Mutex<[Pacing; 2]>,
    /// Only changed under the `decision` lock, so store-vs-bump cannot race.
    generation: AtomicU64,
    /// The arm this bot plays (`jev`, `jev_static`) — journal label, set once by `JevLink`.
    arm: std::sync::OnceLock<&'static str>,
}

#[derive(Default)]
struct Breaker {
    hard: u32,
    timeouts: u32,
    open_until: Option<Instant>,
}

struct Shared {
    bucket: Bucket,
    refresh_inflight: usize,
    breaker: Breaker,
    counters: Counters,
}

struct Inner {
    cfg: HubConfig,
    transport: Arc<dyn DecisionTransport>,
    bots: Vec<BotSlot>,
    next_ticket: AtomicU64,
    notify: Notify,
    shared: Mutex<Shared>,
    stop: AtomicBool,
    /// The journal writer's channel (a dedicated thread — never the tick path).
    journal: Option<std::sync::mpsc::Sender<String>>,
    /// Journal lines waiting for their outcome, by ticket.
    journal_open: Mutex<std::collections::HashMap<u64, serde_json::Value>>,
}

/// One picked, admitted ask.
struct Job {
    bot: usize,
    pending: Pending,
    refresh: bool,
    reserved: f64,
    body: String,
}

enum Pick {
    Job(Job),
    /// Something was dropped (budget); scan again at once.
    Again,
    Idle,
}

/// The process-wide hub. Dropping it stops the workers.
pub struct Hub {
    inner: Arc<Inner>,
    workers: Vec<JoinHandle<()>>,
}

/// Seconds to one decimal, rounded in `f64` — rounding in `f32` journals `4.300000190734863`.
fn round1(s: f32) -> f64 {
    (f64::from(s) * 10.0).round() / 10.0
}

/// One finished life, for the journal (Plan 74 T7) — recorded for **every** arm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LifeRecord {
    /// Seconds alive.
    pub secs: f32,
    /// Seconds from spawn to first holding a non-weak weapon, if it ever did.
    pub secs_to_strong: Option<f32>,
    /// It ended in a death (not a level change).
    pub died: bool,
    /// Frags gained in the life.
    pub frags: i32,
    /// The map it was lived on (closed slug; Plan 74: the arena server rotates maps).
    pub map: &'static str,
}

/// A bot's handle for journalling its lives — any brain, as long as the fleet runs a hub.
#[derive(Clone)]
pub struct LifeSink {
    hub: Arc<Hub>,
    arm: &'static str,
}

impl LifeSink {
    /// A sink for a bot of `brain`, if the fleet has a hub.
    pub fn for_bot(hub: &Option<Arc<Hub>>, brain: brain::BrainKind) -> Option<Self> {
        hub.as_ref().map(|h| Self {
            hub: h.clone(),
            arm: brain::brain_tag(brain),
        })
    }

    /// Append one `{"kind":"life",…}` line (a no-op without a journal; never blocks).
    pub fn record(&self, life: &LifeRecord) {
        let Some(tx) = &self.hub.inner.journal else {
            return;
        };
        let line = serde_json::json!({
            "kind": "life", "arm": self.arm, "secs": round1(life.secs),
            "secs_to_strong": life.secs_to_strong.map(round1), "died": life.died,
            "frags": life.frags, "map": life.map,
        });
        let _ = tx.send(line.to_string());
    }
}

impl Hub {
    /// Start `cfg.max_concurrent` workers for `n_bots` slots. Must be called inside a tokio
    /// runtime.
    pub fn start(cfg: HubConfig, n_bots: usize, transport: Arc<dyn DecisionTransport>) -> Self {
        let now = Instant::now();
        let rate = cfg.max_input_tokens_per_s;
        let journal_tx = cfg.journal.clone().map(spawn_journal_writer);
        let inner = Arc::new(Inner {
            bots: (0..n_bots)
                .map(|_| BotSlot {
                    pending: Mutex::new(None),
                    decision: Mutex::new(None),
                    pacing: Mutex::new(Default::default()),
                    generation: AtomicU64::new(0),
                    arm: std::sync::OnceLock::new(),
                })
                .collect(),
            shared: Mutex::new(Shared {
                bucket: Bucket::new(rate, rate, now),
                refresh_inflight: 0,
                breaker: Breaker::default(),
                counters: Counters::default(),
            }),
            cfg,
            transport,
            next_ticket: AtomicU64::new(0),
            notify: Notify::new(),
            stop: AtomicBool::new(false),
            journal: journal_tx,
            journal_open: Mutex::new(std::collections::HashMap::new()),
        });
        let mut workers: Vec<JoinHandle<()>> = (0..inner.cfg.max_concurrent)
            .map(|_| tokio::spawn(worker(inner.clone())))
            .collect();
        workers.push(tokio::spawn(fleet_line(inner.clone())));
        Hub { inner, workers }
    }

    /// Label bot `bot`'s arm for the journal (first label wins).
    pub fn set_arm(&self, bot: usize, arm: &'static str) {
        let _ = self.inner.bots[bot].arm.set(arm);
    }

    /// The `DecisionSource` handle for bot `bot` (an index below `n_bots`).
    pub fn handle(&self, bot: usize) -> Arc<dyn DecisionSource> {
        assert!(bot < self.inner.bots.len(), "bot index {bot} out of range");
        Arc::new(BotHandle {
            inner: self.inner.clone(),
            bot,
        })
    }

    /// A level change for `bot`: bump its generation (in-flight answers will be discarded) and
    /// clear its pending ask and stored decision.
    pub fn bump_generation(&self, bot: usize) {
        let slot = &self.inner.bots[bot];
        {
            let mut d = lock(&slot.decision);
            slot.generation.fetch_add(1, Ordering::Relaxed);
            *d = None;
        }
        *lock(&slot.pending) = None;
    }

    /// A snapshot of the counters.
    pub fn counters(&self) -> Counters {
        lock(&self.inner.shared).counters
    }

    /// Stop the workers (they exit at their next wake-up).
    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
        self.inner.notify.notify_waiters();
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        self.stop();
        for w in &self.workers {
            w.abort();
        }
    }
}

/// One bot's link to the hub: its `DecisionSource` handle and its level-change hook. Only
/// `jev` bots get one — `jev_static` always reads a never-deciding source.
#[derive(Clone)]
pub struct JevLink {
    hub: Arc<Hub>,
    bot: usize,
}

impl JevLink {
    /// The link for bot index `bot`, if the hub is running and the bot runs the `jev` brain.
    /// A `jev` bot always gets one; a `jev_static` bot only in shadow mode (it then asks and
    /// journals but never steers — `build_brain_with_decider` builds it as a shadow brain).
    pub fn for_bot(hub: &Option<Arc<Hub>>, brain: brain::BrainKind, bot: usize) -> Option<Self> {
        let hub = hub
            .as_ref()
            .filter(|h| wants_hub(brain, h.inner.cfg.shadow))?;
        hub.set_arm(bot, brain::brain_tag(brain));
        Some(Self {
            hub: hub.clone(),
            bot,
        })
    }

    /// This bot's decision source (for `build_brain_with_decider`, once per session).
    pub fn source(&self) -> Arc<dyn DecisionSource> {
        self.hub.handle(self.bot)
    }

    /// The server changed level: discard this bot's in-flight answer, pending ask and decision.
    pub fn level_changed(&self) {
        self.hub.bump_generation(self.bot);
    }
}

/// A bot's view of the hub.
struct BotHandle {
    inner: Arc<Inner>,
    bot: usize,
}

impl DecisionSource for BotHandle {
    fn latest(&self) -> Option<AppliedDecision> {
        let d = lock(&self.inner.bots[self.bot].decision);
        d.and_then(|(dec, at)| (at.elapsed() < self.inner.cfg.ttl).then_some(dec))
    }

    fn submit(&self, ask: Ask) -> Option<TicketId> {
        self.inner.submit(self.bot, ask)
    }

    fn refresh_due(&self, key: QuestionKey) -> bool {
        let p = &lock(&self.inner.bots[self.bot].pacing)[pace_idx(key)];
        p.last_ask.is_none_or(|t| t.elapsed() >= p.dwell)
    }

    fn outcome(&self, ticket: TicketId, o: brain::jev::source::Outcome) {
        let Some(tx) = &self.inner.journal else {
            return;
        };
        let Some(mut line) = lock(&self.inner.journal_open).remove(&ticket.0) else {
            return;
        };
        line["outcome"] = serde_json::json!({
            "secs": round1(o.secs), "died": o.died, "hp_delta": o.hp_delta,
            "armor_delta": o.armor_delta, "frags_delta": o.frags_delta,
            "damage_taken": o.damage_taken, "q3_retreat": o.q3_retreat, "biased": o.biased,
        });
        if let Some(ob) = o.objective {
            // Plan 74 T7: what the bot acted on (the model's or the rule's), and what followed.
            line["outcome"]["objective"] = serde_json::json!({
                "applied": ob.applied, "rule": ob.rule, "arrived": ob.arrived,
                "secs_to_arrive": ob.secs_to_arrive.map(round1),
                "upgraded": ob.upgraded, "released": ob.released,
            });
        }
        let _ = tx.send(line.to_string());
    }

    fn ack(&self, ticket: TicketId, outcome: ApplyOutcome) {
        {
            let mut sh = lock(&self.inner.shared);
            match outcome {
                ApplyOutcome::Applied => sh.counters.applied += 1,
                ApplyOutcome::Shadowed => sh.counters.shadowed += 1,
                ApplyOutcome::Rejected(_) => sh.counters.rejected += 1,
            }
        }
        match outcome {
            ApplyOutcome::Applied => {
                tracing::debug!(bot = self.bot, ticket = ticket.0, "EVT jev_applied")
            }
            ApplyOutcome::Shadowed => {
                tracing::debug!(bot = self.bot, ticket = ticket.0, "EVT jev_shadowed")
            }
            ApplyOutcome::Rejected(why) => tracing::info!(
                bot = self.bot,
                ticket = ticket.0,
                why = why.slug(),
                "EVT jev_reject"
            ),
        }
    }
}

impl Inner {
    fn submit(&self, bot: usize, ask: Ask) -> Option<TicketId> {
        let slot = &self.bots[bot];
        let now = Instant::now();
        let key = ask
            .questions
            .first()
            .map_or(QuestionKey::Posture, |q| q.key);
        let ticket = {
            let mut clocks = lock(&slot.pacing);
            let p = &mut clocks[pace_idx(key)];
            let gap = p.last_ask.map(|t| now.saturating_duration_since(t));
            let allowed = match gap {
                None => true,
                Some(g) if ask.trig.is_event() => g >= p.floor,
                Some(g) => g >= p.dwell,
            };
            if !allowed {
                drop(clocks);
                lock(&self.shared).counters.paced += 1;
                return None;
            }
            let n = self.next_ticket.fetch_add(1, Ordering::Relaxed) + 1;
            p.last_ask = Some(now);
            p.floor = jittered(self.cfg.event_floor, self.cfg.jitter_max, unit(bot, n, 1));
            p.dwell = jittered(self.cfg.min_dwell, self.cfg.jitter_max, unit(bot, n, 2));
            TicketId(n)
        };
        let generation = slot.generation.load(Ordering::Relaxed);
        *lock(&slot.pending) = Some(Pending {
            ticket,
            ask,
            created: now,
            generation,
        });
        {
            let mut sh = lock(&self.shared);
            if is_shadow_ask(slot.arm.get(), key) {
                sh.counters.shadow_asked += 1;
            } else {
                sh.counters.asked += 1;
            }
        }
        self.notify.notify_waiters();
        Some(ticket)
    }

    /// Choose, admit and take the next ask (see the module doc for the policy).
    fn pick(&self, now: Instant) -> Pick {
        let mut sh = lock(&self.shared);
        if let Some(until) = sh.breaker.open_until {
            if now < until {
                for slot in &self.bots {
                    if lock(&slot.pending).take().is_some() {
                        sh.counters.circuit += 1;
                    }
                }
                return Pick::Idle;
            }
            sh.breaker = Breaker::default();
        }
        // Expire stale pendings; find the oldest event and the oldest refresh.
        let mut best_event: Option<(Instant, usize)> = None;
        let mut best_refresh: Option<(Instant, usize)> = None;
        for (i, slot) in self.bots.iter().enumerate() {
            let mut p = lock(&slot.pending);
            let Some(pend) = p.as_ref() else { continue };
            if now.saturating_duration_since(pend.created) > self.cfg.snapshot_ttl {
                *p = None;
                sh.counters.skipped_no_capacity += 1;
                continue;
            }
            let best = if pend.ask.trig.is_event() {
                &mut best_event
            } else {
                &mut best_refresh
            };
            if best.is_none_or(|(t, _)| pend.created < t) {
                *best = Some((pend.created, i));
            }
        }
        let refresh_ok = sh.refresh_inflight < self.cfg.refresh_max_inflight;
        let chosen = best_event.or(best_refresh.filter(|_| refresh_ok));
        let Some((_, bot)) = chosen else {
            return Pick::Idle;
        };
        let Some(pending) = lock(&self.bots[bot].pending).take() else {
            return Pick::Again;
        };
        let refresh = !pending.ask.trig.is_event();
        let state = render_state(&pending.ask.state, self.cfg.max_state_tokens);
        let body = render_request(&state, &pending.ask.questions);
        let reserved = est_tokens(&body) as f64;
        if !sh.bucket.try_reserve(now, reserved) {
            sh.counters.budget += 1;
            return Pick::Again;
        }
        if refresh {
            sh.refresh_inflight += 1;
        }
        sh.counters.admitted += 1;
        Pick::Job(Job {
            bot,
            pending,
            refresh,
            reserved,
            body,
        })
    }

    async fn run(&self, job: Job) {
        let t0 = Instant::now();
        let res = self.transport.post(job.body.clone()).await;
        let lat_ms = t0.elapsed().as_millis();
        let validated = res.and_then(|bytes| {
            validate(
                &bytes,
                &job.pending.ask.questions,
                &self.cfg.model,
                &self.cfg.gate,
            )
            .map_err(PostError::Malformed)
        });
        {
            let mut sh = lock(&self.shared);
            if job.refresh {
                sh.refresh_inflight -= 1;
            }
            match &validated {
                Ok(v) => {
                    sh.breaker.hard = 0;
                    sh.breaker.timeouts = 0;
                    let actual = v.input_tokens.map_or(job.reserved, f64::from);
                    sh.bucket.true_up(job.reserved, actual);
                }
                Err(e) => self.note_failure(&mut sh, *e),
            }
        }
        match validated {
            Ok(v) => self.store(job.bot, &job.pending, &v, lat_ms),
            Err(e) => tracing::debug!(
                bot = job.bot,
                ticket = job.pending.ticket.0,
                trig = job.pending.ask.trig.slug(),
                err = ?e,
                lat_ms,
                "EVT jev_drop"
            ),
        }
        self.notify.notify_waiters();
    }

    fn note_failure(&self, sh: &mut Shared, e: PostError) {
        let (hard, cause) = match e {
            PostError::Transport(TransportErr::Timeout) => {
                sh.counters.transport += 1;
                sh.breaker.timeouts += 1;
                (false, "timeout".to_string())
            }
            PostError::Transport(t) => {
                sh.counters.transport += 1;
                sh.breaker.hard += 1;
                (true, t.slug().into_owned())
            }
            PostError::Malformed(k) => {
                sh.counters.malformed += 1;
                if k == MalformedKind::ModelMismatch {
                    sh.breaker.hard += 1;
                }
                (k == MalformedKind::ModelMismatch, k.slug().to_string())
            }
        };
        let trips = sh.breaker.hard >= self.cfg.breaker_hard
            || sh.breaker.timeouts >= self.cfg.breaker_timeouts;
        if trips && sh.breaker.open_until.is_none() {
            sh.breaker.open_until = Some(Instant::now() + self.cfg.breaker_open);
            sh.counters.breaker_opened += 1;
            tracing::error!(
                err = %cause,
                hard_tier = hard,
                open_s = self.cfg.breaker_open.as_secs_f32(),
                "EVT jev_breaker_open — the decision endpoint is failing; bots run pure q3"
            );
        }
    }

    fn store(&self, bot: usize, pending: &Pending, v: &Validated, lat_ms: u128) {
        let slot = &self.bots[bot];
        let mut d = lock(&slot.decision);
        if slot.generation.load(Ordering::Relaxed) != pending.generation {
            drop(d);
            lock(&self.shared).counters.stale_generation += 1;
            return;
        }
        let choice = |key: QuestionKey| {
            v.results
                .iter()
                .find(|(k, _)| *k == key)
                .and_then(|(_, r)| match r {
                    QuestionResult::Choice(Decision::Apply(i)) => u8::try_from(*i).ok(),
                    _ => None,
                })
        };
        let posture_asked = choice(QuestionKey::Posture).is_some();
        let suppressed = (posture_asked && !self.cfg.posture_live)
            || (self.cfg.disengage_needs_hurt
                && posture_label(pending, choice(QuestionKey::Posture))
                    == Some(brain::jev::question::DISENGAGE)
                && !bot_is_hurt(pending));
        let map = match pending.ask.state.get(FieldKey::Map) {
            Some(Field::Map(m)) => m.map,
            _ => MapId::Other,
        };
        *d = Some((
            AppliedDecision {
                ticket: pending.ticket,
                generation: pending.generation,
                map,
                objective: choice(QuestionKey::Objective),
                posture: choice(QuestionKey::Posture).filter(|_| !suppressed),
            },
            Instant::now(),
        ));
        drop(d);
        let decisive = v
            .results
            .iter()
            .any(|(_, r)| matches!(r, QuestionResult::Choice(Decision::Apply(_))));
        {
            let mut sh = lock(&self.shared);
            sh.counters.stored += 1;
            if decisive {
                sh.counters.decisive += 1;
            } else {
                sh.counters.no_signal += 1;
            }
        }
        log_decision(bot, pending, v, lat_ms);
        if self.journal.is_some() {
            let arm = slot.arm.get().copied().unwrap_or("?");
            let state = render_state(&pending.ask.state, self.cfg.max_state_tokens).text;
            let mut line = journal_entry(bot, arm, pending, v, &state, lat_ms);
            if suppressed {
                line["answers"][0]["gate"] = "suppressed".into();
            }
            lock(&self.journal_open).insert(pending.ticket.0, line);
        }
    }
}

/// Does a bot of `brain` talk to the hub? `jev` and `jev_rule` always; `jev_static` only in
/// shadow mode. The one place this is decided (supervisor gates and `JevLink::for_bot`).
pub fn wants_hub(brain: brain::BrainKind, shadow: bool) -> bool {
    match brain {
        brain::BrainKind::Jev | brain::BrainKind::JevRule => true,
        brain::BrainKind::JevStatic => shadow,
        _ => false,
    }
}

/// Is an ask from `arm` with first question `key` a shadow ask (followed, never applied)?
/// `jev_static` asks only as a shadow; `jev_rule`'s objective asks are shadows (the rule picks).
fn is_shadow_ask(arm: Option<&&'static str>, key: QuestionKey) -> bool {
    match arm.copied() {
        Some("jev_static") => true,
        Some("jev_rule") => key == QuestionKey::Objective,
        _ => false,
    }
}

/// Which pacing clock an ask of `key` uses (an ask's first question decides).
fn pace_idx(key: QuestionKey) -> usize {
    match key {
        QuestionKey::Posture => 0,
        QuestionKey::Objective => 1,
    }
}

/// The posture label an answer's option index stands for.
fn posture_label(pending: &Pending, idx: Option<u8>) -> Option<&'static str> {
    let spec = pending
        .ask
        .questions
        .iter()
        .find(|q| q.key == QuestionKey::Posture)?;
    match &spec.kind {
        QuestionKind::Choice(opts) => opts.get(usize::from(idx?)).map(|o| o.label),
        _ => None,
    }
}

/// Is the asking bot hurt (health band `crit` or `lo`) in the state it asked about?
fn bot_is_hurt(pending: &Pending) -> bool {
    use brain::jev::state::HealthBand;
    matches!(
        pending.ask.state.get(FieldKey::Self_),
        Some(Field::Self_(s)) if matches!(s.band, HealthBand::Crit | HealthBand::Lo)
    )
}

/// The journal writer: a dedicated thread appending lines to `path` (created with its parent).
fn spawn_journal_writer(path: std::path::PathBuf) -> std::sync::mpsc::Sender<String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::Write as _;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        else {
            tracing::error!(path = %path.display(), "jev journal: cannot open; journal disabled");
            return;
        };
        tracing::info!(path = %path.display(), "jev journal open");
        for line in rx {
            let _ = writeln!(f, "{line}");
        }
    });
    tx
}

/// A journal line, waiting for its outcome: who asked what about which state, and the answer.
fn journal_entry(
    bot: usize,
    arm: &str,
    pending: &Pending,
    v: &Validated,
    state: &str,
    lat_ms: u128,
) -> serde_json::Value {
    let questions: Vec<serde_json::Value> = v
        .results
        .iter()
        .zip(&v.stats)
        .map(|((key, r), st)| {
            let spec = pending.ask.questions.iter().find(|q| q.key == *key);
            let top = st.and_then(|st| match spec.map(|q| &q.kind) {
                Some(QuestionKind::Choice(opts)) => opts.get(st.top).map(|o| o.label),
                _ => None,
            });
            serde_json::json!({
                "q": key.wire_key(), "top": top,
                "margin": st.map(|s| (s.margin * 1000.0).round() / 1000.0),
                "n_eff": st.map(|s| (s.n_eff * 100.0).round() / 100.0),
                "gate": if matches!(r, QuestionResult::Choice(Decision::Apply(_))) { "apply" } else { "no_signal" },
            })
        })
        .collect();
    let map = match pending.ask.state.get(FieldKey::Map) {
        Some(Field::Map(m)) => m.map.slug(),
        _ => "other",
    };
    serde_json::json!({
        "ts": time::OffsetDateTime::now_utc().unix_timestamp(),
        "bot": bot, "arm": arm, "ticket": pending.ticket.0, "trig": pending.ask.trig.slug(),
        "map": map, "schema": brain::jev::state::STATE_SCHEMA_VERSION,
        "state": state, "answers": questions, "tok": v.input_tokens, "lat_ms": lat_ms,
    })
}

/// One `EVT jev_decision` line per question of a stored answer.
fn log_decision(bot: usize, pending: &Pending, v: &Validated, lat_ms: u128) {
    for ((key, r), st) in v.results.iter().zip(&v.stats) {
        let spec = pending.ask.questions.iter().find(|q| q.key == *key);
        let top = st.and_then(|st| match spec.map(|q| &q.kind) {
            Some(QuestionKind::Choice(opts)) => opts.get(st.top).map(|o| o.label),
            _ => None,
        });
        tracing::info!(
            bot,
            ticket = pending.ticket.0,
            trig = pending.ask.trig.slug(),
            q = key.wire_key(),
            top = top.unwrap_or("-"),
            margin = format!("{:.3}", st.map_or(0.0, |s| s.margin)),
            n_eff = format!("{:.2}", st.map_or(0.0, |s| s.n_eff)),
            gate = if matches!(r, QuestionResult::Choice(Decision::Apply(_))) {
                "apply"
            } else {
                "no_signal"
            },
            tok = v.input_tokens.unwrap_or(0),
            lat_ms,
            "EVT jev_decision"
        );
    }
}

/// `EVT jev_fleet` every 10 s while anything has been asked — the continuous line that shows
/// a decision-rate collapse while it happens, not only in the final report.
async fn fleet_line(inner: Arc<Inner>) {
    let mut iv = tokio::time::interval(Duration::from_secs(10));
    iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    iv.tick().await;
    loop {
        iv.tick().await;
        if inner.stop.load(Ordering::Relaxed) {
            return;
        }
        let (c, inflight) = {
            let sh = lock(&inner.shared);
            (sh.counters, sh.refresh_inflight)
        };
        if c.asked + c.shadow_asked == 0 {
            continue;
        }
        tracing::info!(
            asked = c.asked,
            admitted = c.admitted,
            stored = c.stored,
            decisive = c.decisive,
            applied = c.applied,
            rejected = c.rejected,
            paced = c.paced,
            skipped = c.skipped_no_capacity,
            budget = c.budget,
            circuit = c.circuit,
            transport = c.transport,
            malformed = c.malformed,
            shadow_asked = c.shadow_asked,
            shadowed = c.shadowed,
            refresh_inflight = inflight,
            applied_rate = format!("{:.2}", c.applied_rate()),
            "EVT jev_fleet"
        );
    }
}

async fn worker(inner: Arc<Inner>) {
    loop {
        let notified = inner.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if inner.stop.load(Ordering::Relaxed) {
            return;
        }
        match inner.pick(Instant::now()) {
            Pick::Job(job) => inner.run(job).await,
            Pick::Again => {}
            Pick::Idle => {
                // Also wake when an open breaker closes, so asks submitted meanwhile are seen.
                let _ = tokio::time::timeout(Duration::from_secs(1), notified).await;
            }
        }
    }
}

/// A deterministic `[0, 1)` draw for (bot, ticket, stream) — splitmix64. Phase decorrelation
/// needs spread, not secrecy, and there is no `rand` in the lockfile.
fn unit(bot: usize, ticket: u64, stream: u64) -> f64 {
    let mut z = (bot as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(ticket.wrapping_mul(0xBF58_476D_1CE4_E5B9))
        .wrapping_add(stream);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// `base × [1, jitter_max]` — one-sided, so the provable worst-case rate is `1/base`.
fn jittered(base: Duration, jitter_max: f64, u: f64) -> Duration {
    base.mul_f64(1.0 + (jitter_max - 1.0).max(0.0) * u)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
