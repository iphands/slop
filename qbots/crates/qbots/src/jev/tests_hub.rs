//! Plan 72 T5 tests: the hub's mechanisms under a **paused** tokio clock with a scripted
//! transport (no sockets). The token bucket's own algorithm is tested in `bucket.rs` with an
//! explicit clock; here it is exercised through the hub.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use brain::jev::decision::TransportErr;
use brain::jev::question::{ChoiceOption, Phrase, QuestionKey, QuestionKind, QuestionSpec};
use brain::jev::source::{ApplyOutcome, Ask, BrainReject, DecisionSource, Trigger};
use brain::jev::state::{DecisionState, Field, HealthBand, LoadoutGap, MapFacts, MapId, SelfFacts};

use super::hub::{Hub, HubConfig};
use super::net::PostError;
use super::transport::{BoxFut, DecisionTransport};

const MODEL: &str = "h2oai/h2o-lightning-4b";

type Reply = (Duration, Result<Vec<u8>, PostError>);

/// A scripted transport: replies are popped in order (default: a decisive answer after 10 ms);
/// every request body is recorded; in-flight requests are counted.
#[derive(Clone, Default)]
struct Scripted(Arc<ScriptState>);

#[derive(Default)]
struct ScriptState {
    replies: Mutex<VecDeque<Reply>>,
    bodies: Mutex<Vec<String>>,
    inflight: AtomicUsize,
}

impl Scripted {
    fn push(&self, delay_ms: u64, r: Result<&str, PostError>) {
        let r = r.map(|s| s.as_bytes().to_vec());
        self.0
            .replies
            .lock()
            .expect("replies lock")
            .push_back((Duration::from_millis(delay_ms), r));
    }
    fn bodies(&self) -> Vec<String> {
        self.0.bodies.lock().expect("bodies lock").clone()
    }
    fn inflight(&self) -> usize {
        self.0.inflight.load(Ordering::SeqCst)
    }
}

impl DecisionTransport for Scripted {
    fn post(&self, body: String) -> BoxFut<Result<Vec<u8>, PostError>> {
        let st = self.0.clone();
        Box::pin(async move {
            st.bodies.lock().expect("bodies lock").push(body);
            let (delay, r) = st
                .replies
                .lock()
                .expect("replies lock")
                .pop_front()
                .unwrap_or((Duration::from_millis(10), Ok(decisive().into_bytes())));
            st.inflight.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(delay).await;
            st.inflight.fetch_sub(1, Ordering::SeqCst);
            r
        })
    }
}

fn answer(pa: f32, pb: f32) -> String {
    let choice = if pa >= pb { "a" } else { "b" };
    format!(
        r#"{{"model":"{MODEL}","answers":{{"objective":{{"type":"choice","choice":"{choice}","probabilities":{{"a":{pa},"b":{pb}}}}}}},"usage":{{"input_tokens":100,"output_tokens":0}}}}"#
    )
}

fn decisive() -> String {
    answer(0.9, 0.1)
}

/// An ask whose rendered state carries a unique `hp` so its body is identifiable.
fn ask(hp: u32, trig: Trigger) -> Ask {
    Ask {
        state: DecisionState::from_fields(vec![
            Field::Self_(SelfFacts {
                hp,
                band: HealthBand::of(hp as i32),
                armor: 0,
                held: None,
                ammo: 0,
                ammo_low: true,
            }),
            Field::Map(MapFacts {
                map: MapId::Q2dm1,
                gap: LoadoutGap::None,
            }),
        ]),
        questions: vec![QuestionSpec {
            key: QuestionKey::Objective,
            instructions: "Pick one.",
            kind: QuestionKind::Choice(vec![
                ChoiceOption {
                    label: "a",
                    desc: Phrase::lit("option a"),
                },
                ChoiceOption {
                    label: "b",
                    desc: Phrase::lit("option b"),
                },
            ]),
        }],
        trig,
    }
}

fn has_hp(body: &str, hp: u32) -> bool {
    body.contains(&format!("hp:{hp}("))
}

/// Test defaults: no pacing floor, effectively unlimited budget.
fn cfg() -> HubConfig {
    HubConfig {
        event_floor: Duration::ZERO,
        max_input_tokens_per_s: 1e9,
        ..HubConfig::default()
    }
}

fn start(cfg: HubConfig, n: usize, t: &Scripted) -> (Hub, Vec<Arc<dyn DecisionSource>>) {
    let hub = Hub::start(cfg, n, Arc::new(t.clone()));
    let handles = (0..n).map(|i| hub.handle(i)).collect();
    (hub, handles)
}

async fn settle(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

#[tokio::test(start_paused = true)]
async fn jev_hub_second_submit_replaces_the_pending_ask() {
    let t = Scripted::default();
    let (hub, h) = start(cfg(), 1, &t);
    let first = h[0].submit(ask(11, Trigger::Damage)).expect("accepted");
    let second = h[0].submit(ask(22, Trigger::Damage)).expect("accepted");
    assert_ne!(first, second, "a replacement gets a new ticket");
    settle(100).await;
    let bodies = t.bodies();
    assert_eq!(bodies.len(), 1, "one ask in flight per bot, never a queue");
    assert!(has_hp(&bodies[0], 22), "newest wins");
    assert_eq!(h[0].latest().map(|d| d.ticket), Some(second));
    assert_eq!(hub.counters().asked, 2);
}

#[tokio::test(start_paused = true)]
async fn jev_hub_stale_pending_is_dropped_at_pick_and_never_sent() {
    let t = Scripted::default();
    for _ in 0..3 {
        t.push(5000, Ok(&decisive()));
    }
    let (hub, h) = start(cfg(), 4, &t);
    for (i, handle) in h.iter().take(3).enumerate() {
        handle.submit(ask(10 + i as u32, Trigger::Damage));
    }
    settle(10).await;
    assert_eq!(t.inflight(), 3, "all workers busy");
    h[3].submit(ask(99, Trigger::Damage));
    settle(6000).await;
    assert_eq!(hub.counters().skipped_no_capacity, 1);
    assert!(
        !t.bodies().iter().any(|b| has_hp(b, 99)),
        "the dead-world ask was never sent"
    );
}

#[tokio::test(start_paused = true)]
async fn jev_hub_oldest_pending_is_served_first() {
    let t = Scripted::default();
    t.push(1000, Ok(&decisive()));
    t.push(5000, Ok(&decisive()));
    t.push(5000, Ok(&decisive()));
    t.push(5000, Ok(&decisive())); // the 4th request holds its worker, so only one is picked
    let c = HubConfig {
        snapshot_ttl: Duration::from_secs(10),
        ..cfg()
    };
    let (_hub, h) = start(c, 5, &t);
    for (i, handle) in h.iter().take(3).enumerate() {
        handle.submit(ask(10 + i as u32, Trigger::Damage));
    }
    settle(10).await;
    h[3].submit(ask(33, Trigger::Damage));
    settle(100).await;
    h[4].submit(ask(44, Trigger::Damage));
    settle(1000).await; // the 1 s request has finished; one worker freed
    let bodies = t.bodies();
    assert_eq!(bodies.len(), 4);
    assert!(has_hp(&bodies[3], 33), "the older pending ask went first");
}

#[tokio::test(start_paused = true)]
async fn jev_hub_refresh_is_capped_and_events_go_first() {
    let t = Scripted::default();
    for _ in 0..4 {
        t.push(5000, Ok(&decisive()));
    }
    let (_hub, h) = start(cfg(), 4, &t);
    for (i, handle) in h.iter().take(3).enumerate() {
        handle.submit(ask(10 + i as u32, Trigger::Refresh));
    }
    settle(10).await;
    assert_eq!(t.inflight(), 2, "refresh never holds more than 2 workers");
    h[3].submit(ask(77, Trigger::Respawn));
    settle(10).await;
    assert_eq!(t.inflight(), 3, "an event takes the third worker at once");
    assert!(has_hp(&t.bodies()[2], 77));
}

#[tokio::test(start_paused = true)]
async fn jev_hub_event_is_picked_before_an_older_refresh() {
    let t = Scripted::default();
    t.push(1000, Ok(&decisive()));
    let c = HubConfig {
        max_concurrent: 1,
        snapshot_ttl: Duration::from_secs(10),
        ..cfg()
    };
    let (_hub, h) = start(c, 3, &t);
    h[0].submit(ask(10, Trigger::Damage));
    settle(10).await;
    h[1].submit(ask(11, Trigger::Refresh));
    settle(100).await;
    h[2].submit(ask(12, Trigger::Damage));
    settle(1000).await;
    assert!(
        has_hp(&t.bodies()[1], 12),
        "the newer event beat the older refresh"
    );
}

#[tokio::test(start_paused = true)]
async fn jev_hub_pacing_floor_refresh_dwell_and_event_reset() {
    let t = Scripted::default();
    let c = HubConfig {
        event_floor: Duration::from_secs(5),
        min_dwell: Duration::from_secs(15),
        ..cfg()
    };
    let (hub, h) = start(c, 1, &t);
    assert!(h[0].refresh_due(), "a bot that never asked is due");
    assert!(h[0].submit(ask(1, Trigger::Respawn)).is_some());
    assert_eq!(
        h[0].submit(ask(2, Trigger::Damage)),
        None,
        "inside the event floor"
    );
    assert!(!h[0].refresh_due());
    settle(6600).await; // > 5 s × 1.3
    assert!(
        h[0].submit(ask(3, Trigger::Damage)).is_some(),
        "floor passed"
    );
    settle(10_000).await; // 16.6 s after the first ask, 10 s after the second
    assert!(!h[0].refresh_due(), "the event reset the refresh timer");
    assert_eq!(h[0].submit(ask(4, Trigger::Refresh)), None);
    settle(10_000).await; // 20 s after the second ask > 15 s × 1.3
    assert!(h[0].refresh_due());
    assert!(h[0].submit(ask(5, Trigger::Refresh)).is_some());
    assert_eq!(hub.counters().paced, 2);
}

#[tokio::test(start_paused = true)]
async fn jev_hub_breaker_opens_on_hard_failures_and_recovers() {
    let t = Scripted::default();
    for _ in 0..3 {
        t.push(10, Err(PostError::Transport(TransportErr::Refused)));
    }
    let (hub, h) = start(cfg(), 1, &t);
    for i in 0..3 {
        h[0].submit(ask(i, Trigger::Damage));
        settle(50).await;
    }
    assert_eq!(hub.counters().breaker_opened, 1);
    h[0].submit(ask(50, Trigger::Damage));
    settle(50).await;
    assert_eq!(t.bodies().len(), 3, "an open breaker sends nothing");
    assert_eq!(hub.counters().circuit, 1);
    settle(5000).await;
    h[0].submit(ask(60, Trigger::Damage));
    settle(100).await;
    assert_eq!(t.bodies().len(), 4, "closed again after breaker_open");
    assert!(h[0].latest().is_some());
}

#[tokio::test(start_paused = true)]
async fn jev_hub_isolated_timeouts_do_not_open_the_breaker() {
    let t = Scripted::default();
    for _ in 0..2 {
        t.push(10, Err(PostError::Transport(TransportErr::Timeout)));
    }
    t.push(10, Ok(&decisive()));
    for _ in 0..2 {
        t.push(10, Err(PostError::Transport(TransportErr::Timeout)));
    }
    let (hub, h) = start(cfg(), 1, &t);
    for i in 0..5 {
        h[0].submit(ask(i, Trigger::Damage));
        settle(50).await;
    }
    assert_eq!(hub.counters().breaker_opened, 0);
    assert_eq!(hub.counters().transport, 4);
    assert_eq!(t.bodies().len(), 5);
}

#[tokio::test(start_paused = true)]
async fn jev_hub_model_mismatch_counts_toward_the_hard_tier() {
    let t = Scripted::default();
    let wrong = decisive().replace(MODEL, "someone/else");
    for _ in 0..3 {
        t.push(10, Ok(&wrong));
    }
    let (hub, h) = start(cfg(), 1, &t);
    for i in 0..3 {
        h[0].submit(ask(i, Trigger::Damage));
        settle(50).await;
    }
    assert_eq!(hub.counters().malformed, 3);
    assert_eq!(hub.counters().breaker_opened, 1);
}

#[tokio::test(start_paused = true)]
async fn jev_hub_generation_bump_discards_the_in_flight_answer() {
    let t = Scripted::default();
    t.push(1000, Ok(&decisive()));
    let (hub, h) = start(cfg(), 1, &t);
    h[0].submit(ask(1, Trigger::MapChange));
    settle(10).await;
    hub.bump_generation(0);
    settle(2000).await;
    assert_eq!(h[0].latest(), None);
    assert_eq!(hub.counters().stale_generation, 1);
    assert_eq!(hub.counters().stored, 0);
}

#[tokio::test(start_paused = true)]
async fn jev_hub_gated_off_answer_supersedes_and_transport_error_stores_nothing() {
    let t = Scripted::default();
    t.push(10, Ok(&decisive()));
    t.push(10, Ok(&answer(0.55, 0.45)));
    t.push(10, Err(PostError::Transport(TransportErr::Refused)));
    let (_hub, h) = start(cfg(), 1, &t);
    let t1 = h[0].submit(ask(1, Trigger::Damage)).expect("accepted");
    settle(50).await;
    let d1 = h[0].latest().expect("stored");
    assert_eq!(
        (d1.ticket, d1.objective, d1.map),
        (t1, Some(0), MapId::Q2dm1)
    );
    let t2 = h[0].submit(ask(2, Trigger::Damage)).expect("accepted");
    settle(50).await;
    let d2 = h[0].latest().expect("stored");
    assert_eq!(
        (d2.ticket, d2.objective),
        (t2, None),
        "flat answer supersedes as None"
    );
    h[0].submit(ask(3, Trigger::Damage));
    settle(50).await;
    assert_eq!(
        h[0].latest().map(|d| d.ticket),
        Some(t2),
        "a failed request stores nothing"
    );
}

#[tokio::test(start_paused = true)]
async fn jev_hub_stored_decision_expires_after_ttl() {
    let t = Scripted::default();
    let (_hub, h) = start(cfg(), 1, &t);
    h[0].submit(ask(1, Trigger::Damage));
    settle(50).await;
    assert!(h[0].latest().is_some());
    settle(24_000).await;
    assert_eq!(h[0].latest(), None);
}

#[tokio::test(start_paused = true)]
async fn jev_hub_budget_refuses_when_overdrawn() {
    let t = Scripted::default();
    let c = HubConfig {
        max_input_tokens_per_s: 10.0,
        ..cfg()
    };
    let (hub, h) = start(c, 2, &t);
    h[0].submit(ask(1, Trigger::Damage));
    settle(50).await;
    h[1].submit(ask(2, Trigger::Damage));
    settle(50).await;
    assert_eq!(
        t.bodies().len(),
        1,
        "the reservation overdrew a 10-token bucket"
    );
    assert_eq!(hub.counters().budget, 1);
}

#[tokio::test(start_paused = true)]
async fn jev_hub_ack_counts_applied_and_rejected() {
    let t = Scripted::default();
    let (hub, h) = start(cfg(), 1, &t);
    let tk = h[0].submit(ask(1, Trigger::Damage)).expect("accepted");
    settle(50).await;
    h[0].ack(tk, ApplyOutcome::Applied);
    h[0].ack(tk, ApplyOutcome::Rejected(BrainReject::WrongMap));
    let c = hub.counters();
    assert_eq!((c.stored, c.applied, c.rejected), (1, 1, 1));
}

#[tokio::test(start_paused = true)]
async fn jev_hub_slot_reads_from_a_non_tokio_thread() {
    let t = Scripted::default();
    let (_hub, h) = start(cfg(), 1, &t);
    let tk = h[0].submit(ask(1, Trigger::Damage)).expect("accepted");
    settle(50).await;
    let handle = h[0].clone();
    let seen = std::thread::spawn(move || handle.latest())
        .join()
        .expect("reader thread");
    assert_eq!(seen.map(|d| d.ticket), Some(tk));
}

#[tokio::test(start_paused = true)]
async fn jev_wiring_links_only_jev_bots_and_level_change_clears_the_decision() {
    use super::hub::JevLink;
    use brain::BrainKind;
    let t = Scripted::default();
    let hub = Some(Arc::new(Hub::start(cfg(), 3, Arc::new(t.clone()))));
    assert!(
        JevLink::for_bot(&hub, BrainKind::JevStatic, 0).is_none(),
        "the control reads nothing"
    );
    assert!(JevLink::for_bot(&hub, BrainKind::Quake3, 1).is_none());
    assert!(
        JevLink::for_bot(&None, BrainKind::Jev, 2).is_none(),
        "no hub, no link"
    );
    let link = JevLink::for_bot(&hub, BrainKind::Jev, 2).expect("a jev bot with a hub");
    let src = link.source();
    src.submit(ask(1, Trigger::Respawn)).expect("accepted");
    settle(50).await;
    assert!(src.latest().is_some());
    link.level_changed();
    assert_eq!(
        src.latest(),
        None,
        "a level change drops the old level's decision"
    );
}

#[tokio::test(start_paused = true)]
async fn jev_telemetry_counts_decisive_no_signal_and_the_three_ratios() {
    let t = Scripted::default();
    t.push(10, Ok(&decisive()));
    t.push(10, Ok(&answer(0.55, 0.45)));
    t.push(10, Err(PostError::Transport(TransportErr::Refused)));
    let c = HubConfig {
        event_floor: Duration::from_secs(5),
        ..cfg()
    };
    let (hub, h) = start(c, 1, &t);
    let t1 = h[0].submit(ask(1, Trigger::Damage)).expect("accepted");
    settle(50).await;
    h[0].ack(t1, ApplyOutcome::Applied);
    assert_eq!(h[0].submit(ask(2, Trigger::Damage)), None, "paced");
    settle(6600).await;
    h[0].submit(ask(3, Trigger::Damage)).expect("accepted");
    settle(6600).await;
    h[0].submit(ask(4, Trigger::Damage)).expect("accepted");
    settle(50).await;
    let c = hub.counters();
    assert_eq!((c.asked, c.paced, c.admitted), (3, 1, 3));
    assert_eq!(
        (c.stored, c.decisive, c.no_signal, c.transport),
        (2, 1, 1, 1)
    );
    assert_eq!(c.applied, 1);
    assert!(
        (c.applied_rate() - 1.0 / 3.0).abs() < 1e-9,
        "applied / asked"
    );
    assert!(
        (c.gate_reject_rate() - 0.5).abs() < 1e-9,
        "no_signal / stored"
    );
    assert_eq!(c.admission_drop_rate(), 0.0);
}

#[tokio::test(start_paused = true)]
async fn jev_journal_writes_one_line_per_decision_with_its_outcome() {
    use brain::jev::source::Outcome;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("logs/journal.jsonl");
    let t = Scripted::default();
    let c = HubConfig {
        journal: Some(path.clone()),
        ..cfg()
    };
    let (hub, h) = start(c, 1, &t);
    hub.set_arm(0, "jev");
    let tk = h[0].submit(ask(42, Trigger::Contact)).expect("accepted");
    settle(50).await;
    h[0].outcome(
        tk,
        Outcome {
            secs: 10.0,
            died: true,
            hp_delta: -42,
            damage_taken: 50,
            q3_retreat: Some(false),
            biased: true,
            ..Default::default()
        },
    );
    h[0].outcome(tk, Outcome::default()); // a second outcome for the same ticket is ignored
                                          // The writer is a real thread: wait (real time) for its line.
    let mut text = String::new();
    for _ in 0..100 {
        text = std::fs::read_to_string(&path).unwrap_or_default();
        if !text.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1);
    let v: serde_json::Value = serde_json::from_str(lines[0]).expect("json");
    assert_eq!(
        (v["arm"].as_str(), v["trig"].as_str()),
        (Some("jev"), Some("contact"))
    );
    assert!(v["state"].as_str().is_some_and(|s| s.contains("hp:42(")));
    assert_eq!(v["answers"][0]["top"], "a");
    assert_eq!(v["answers"][0]["gate"], "apply");
    assert_eq!(v["outcome"]["died"], true);
    assert_eq!(v["outcome"]["hp_delta"], -42);
    assert_eq!(v["schema"], 2);
}

/// A posture ask from a bot at `hp`, and the model's "disengage" answer to it.
fn posture_ask(hp: u32) -> Ask {
    let mut a = ask(hp, Trigger::Contact);
    a.questions = vec![brain::jev::question::posture_question()];
    a
}

fn disengage_answer() -> String {
    format!(
        r#"{{"model":"{MODEL}","answers":{{"posture":{{"type":"choice","choice":"disengage","probabilities":{{"disengage":0.8,"hold":0.2}}}}}},"usage":{{"input_tokens":100}}}}"#
    )
}

#[tokio::test(start_paused = true)]
async fn jev_a2_disengage_needs_hurt_suppresses_it_on_a_healthy_bot() {
    let t = Scripted::default();
    t.push(10, Ok(&disengage_answer()));
    t.push(10, Ok(&disengage_answer()));
    let c = HubConfig {
        disengage_needs_hurt: true,
        ..cfg()
    };
    let (_hub, h) = start(c, 2, &t);
    h[0].submit(posture_ask(100)).expect("accepted"); // healthy
    h[1].submit(posture_ask(20)).expect("accepted"); // crit
    settle(50).await;
    assert_eq!(
        h[0].latest().map(|d| d.posture),
        Some(None),
        "suppressed on a healthy bot"
    );
    assert_eq!(
        h[1].latest().map(|d| d.posture),
        Some(Some(0)),
        "kept on a hurt bot"
    );
}

#[tokio::test(start_paused = true)]
async fn jev_a2_off_by_default_keeps_the_healthy_disengage() {
    let t = Scripted::default();
    t.push(10, Ok(&disengage_answer()));
    let (_hub, h) = start(cfg(), 1, &t);
    h[0].submit(posture_ask(100)).expect("accepted");
    settle(50).await;
    assert_eq!(h[0].latest().map(|d| d.posture), Some(Some(0)));
}
