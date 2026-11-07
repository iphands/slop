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
//! - **Pacing, per bot (at `submit`):** `event_floor` between any two asks, refresh every
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
use brain::jev::question::QuestionKey;
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
    pacing: Mutex<Pacing>,
    /// Only changed under the `decision` lock, so store-vs-bump cannot race.
    generation: AtomicU64,
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

impl Hub {
    /// Start `cfg.max_concurrent` workers for `n_bots` slots. Must be called inside a tokio
    /// runtime.
    pub fn start(cfg: HubConfig, n_bots: usize, transport: Arc<dyn DecisionTransport>) -> Self {
        let now = Instant::now();
        let rate = cfg.max_input_tokens_per_s;
        let inner = Arc::new(Inner {
            bots: (0..n_bots)
                .map(|_| BotSlot {
                    pending: Mutex::new(None),
                    decision: Mutex::new(None),
                    pacing: Mutex::new(Pacing::default()),
                    generation: AtomicU64::new(0),
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
        });
        let workers = (0..inner.cfg.max_concurrent)
            .map(|_| tokio::spawn(worker(inner.clone())))
            .collect();
        Hub { inner, workers }
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

    fn refresh_due(&self) -> bool {
        let p = lock(&self.inner.bots[self.bot].pacing);
        p.last_ask.is_none_or(|t| t.elapsed() >= p.dwell)
    }

    fn ack(&self, _ticket: TicketId, outcome: ApplyOutcome) {
        let mut sh = lock(&self.inner.shared);
        match outcome {
            ApplyOutcome::Applied => sh.counters.applied += 1,
            ApplyOutcome::Rejected(_) => sh.counters.rejected += 1,
        }
    }
}

impl Inner {
    fn submit(&self, bot: usize, ask: Ask) -> Option<TicketId> {
        let slot = &self.bots[bot];
        let now = Instant::now();
        let ticket = {
            let mut p = lock(&slot.pacing);
            let gap = p.last_ask.map(|t| now.saturating_duration_since(t));
            let allowed = match gap {
                None => true,
                Some(g) if ask.trig.is_event() => g >= p.floor,
                Some(g) => g >= p.dwell,
            };
            if !allowed {
                drop(p);
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
        lock(&self.shared).counters.asked += 1;
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
        let res = self.transport.post(job.body.clone()).await;
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
        if let Ok(v) = validated {
            self.store(job.bot, &job.pending, &v);
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

    fn store(&self, bot: usize, pending: &Pending, v: &Validated) {
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
                posture: choice(QuestionKey::Posture),
            },
            Instant::now(),
        ));
        drop(d);
        lock(&self.shared).counters.stored += 1;
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
