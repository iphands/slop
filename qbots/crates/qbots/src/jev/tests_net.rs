//! Plan 72 T4 tests: the hand-rolled HTTP client against a fixture-replaying
//! `tokio::net::TcpListener` (no mock library), and response validation. Every case asserts the
//! **specific** outcome — a panicking test already fails, so "didn't panic" proves nothing.
//!
//! These run on the **real clock**, never `start_paused`: a paused runtime auto-advances time
//! whenever every task is idle, and a task blocked on real socket I/O counts as idle, so the
//! slow-body timeout would fire instantly and pass for the wrong reason.
//!
//! The four `testdata/*.json` bodies are verbatim captures from `cosmo.lan:8710` (2026-11-07 UTC
//! review probes); everything else here is constructed to exercise one failure.

use std::time::Duration;

use brain::jev::decision::{Decision, MalformedKind, NoSignalReason, TransportErr};
use brain::jev::gate::GateThresholds;
use brain::jev::question::{ChoiceOption, Phrase, QuestionKey, QuestionKind, QuestionSpec};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::net::{post_json, Endpoint, PostError, MAX_BODY};
use super::wire::{validate, QuestionResult};

const MODEL: &str = "h2oai/h2o-lightning-4b";
const DECISIVE: &str = include_str!("testdata/posture_binary_decisive.json");
const UNCLEAR: &str = include_str!("testdata/posture_binary_unclear.json");
const THREE_WAY: &str = include_str!("testdata/posture_three_way_flat.json");
const CHAT_501: &str = include_str!("testdata/chat_completions_501.json");
const DEADLINE: Duration = Duration::from_millis(2000);

/// How the one-shot server answers.
enum Reply {
    /// Write these bytes, then close.
    Bytes(Vec<u8>),
    /// Write the first part, sleep, then the rest.
    Slow(Vec<u8>, Duration, Vec<u8>),
}

/// Serve exactly one connection: read the whole request (head + `Content-Length` body), then
/// reply. Returns the endpoint to dial.
async fn serve_once(reply: Reply) -> Endpoint {
    let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = l.local_addr().expect("addr").port();
    tokio::spawn(async move {
        let (mut s, _) = l.accept().await.expect("accept");
        let mut req = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = s.read(&mut tmp).await.expect("read request");
            req.extend_from_slice(&tmp[..n]);
            if n == 0 || request_complete(&req) {
                break;
            }
        }
        match reply {
            Reply::Bytes(b) => {
                let _ = s.write_all(&b).await;
            }
            Reply::Slow(a, d, b) => {
                let _ = s.write_all(&a).await;
                tokio::time::sleep(d).await;
                let _ = s.write_all(&b).await;
            }
        }
    });
    Endpoint {
        host: "127.0.0.1".into(),
        port,
    }
}

fn request_complete(req: &[u8]) -> bool {
    let Some(end) = req.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&req[..end]).to_ascii_lowercase();
    let cl = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    req.len() >= end + 4 + cl
}

fn http(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut v = format!("HTTP/1.1 {status}\r\n{headers}\r\n").into_bytes();
    v.extend_from_slice(body);
    v
}

fn with_length(status: &str, body: &str) -> Vec<u8> {
    http(
        status,
        &format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ),
        body.as_bytes(),
    )
}

fn binary_posture() -> Vec<QuestionSpec> {
    vec![QuestionSpec {
        key: QuestionKey::Posture,
        instructions: "Should this bot keep fighting or retreat to survive?",
        kind: QuestionKind::Choice(vec![
            ChoiceOption {
                label: "disengage",
                desc: Phrase::lit("retreat now to survive and find health"),
            },
            ChoiceOption {
                label: "stay",
                desc: Phrase::lit("keep fighting the enemies"),
            },
        ]),
    }]
}

async fn post(ep: &Endpoint, deadline: Duration) -> Result<Vec<u8>, PostError> {
    post_json(ep, "/v1/systemone", "{}", deadline).await
}

fn val(body: &str, asked: &[QuestionSpec]) -> Result<Vec<QuestionResult>, MalformedKind> {
    validate(body.as_bytes(), asked, MODEL, &GateThresholds::default())
        .map(|v| v.results.into_iter().map(|(_, r)| r).collect())
}

// ── transport: success framings ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn jev_net_content_length_body_round_trips_and_validates() {
    let ep = serve_once(Reply::Bytes(with_length("200 OK", DECISIVE))).await;
    let body = post(&ep, DEADLINE).await.expect("200");
    let v = validate(&body, &binary_posture(), MODEL, &GateThresholds::default()).expect("valid");
    assert_eq!(
        v.results,
        vec![(
            QuestionKey::Posture,
            QuestionResult::Choice(Decision::Apply(0))
        )]
    );
    assert_eq!(v.input_tokens, Some(142));
}

#[tokio::test]
async fn jev_net_chunked_body_is_reassembled() {
    let (a, b) = DECISIVE.split_at(100);
    let chunked = format!(
        "{:x}\r\n{a}\r\n{:x};ext=1\r\n{b}\r\n0\r\nX-Trailer: t\r\n\r\n",
        a.len(),
        b.len()
    );
    let ep = serve_once(Reply::Bytes(http(
        "200 OK",
        "Transfer-Encoding: chunked\r\n",
        chunked.as_bytes(),
    )))
    .await;
    assert_eq!(post(&ep, DEADLINE).await.expect("200"), DECISIVE.as_bytes());
}

#[tokio::test]
async fn jev_net_close_delimited_body_reads_to_eof() {
    let ep = serve_once(Reply::Bytes(http(
        "200 OK",
        "Connection: close\r\n",
        DECISIVE.as_bytes(),
    )))
    .await;
    assert_eq!(post(&ep, DEADLINE).await.expect("200"), DECISIVE.as_bytes());
}

// ── transport: failures ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn jev_net_non_200_statuses_are_transport_http_errors() {
    let bad_type = r#"{"error":{"message":"\"type\" must be one of: choice, score, noul","type":"invalid_request_error"}}"#;
    for (status, body, code) in [
        ("400 Bad Request", bad_type, 400),
        ("501 Not Implemented", CHAT_501, 501),
        ("500 Internal Server Error", "{}", 500),
    ] {
        let ep = serve_once(Reply::Bytes(with_length(status, body))).await;
        assert_eq!(
            post(&ep, DEADLINE).await,
            Err(PostError::Transport(TransportErr::Http(code)))
        );
    }
}

#[tokio::test]
async fn jev_net_truncated_body_is_a_framing_error() {
    let ep = serve_once(Reply::Bytes(http(
        "200 OK",
        "Content-Length: 500\r\n",
        b"{\"model\":",
    )))
    .await;
    assert_eq!(
        post(&ep, DEADLINE).await,
        Err(PostError::Malformed(MalformedKind::Framing))
    );
}

#[tokio::test]
async fn jev_net_incomplete_chunked_body_is_a_framing_error() {
    let ep = serve_once(Reply::Bytes(http(
        "200 OK",
        "Transfer-Encoding: chunked\r\n",
        b"ff\r\nshort",
    )))
    .await;
    assert_eq!(
        post(&ep, DEADLINE).await,
        Err(PostError::Malformed(MalformedKind::Framing))
    );
}

#[tokio::test]
async fn jev_net_close_with_zero_bytes_is_transport_closed() {
    let ep = serve_once(Reply::Bytes(Vec::new())).await;
    assert_eq!(
        post(&ep, DEADLINE).await,
        Err(PostError::Transport(TransportErr::Closed))
    );
}

#[tokio::test]
async fn jev_net_connection_refused() {
    let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = l.local_addr().expect("addr").port();
    drop(l);
    let ep = Endpoint {
        host: "127.0.0.1".into(),
        port,
    };
    assert_eq!(
        post(&ep, DEADLINE).await,
        Err(PostError::Transport(TransportErr::Refused))
    );
}

#[tokio::test]
async fn jev_net_slow_body_hits_the_deadline() {
    let mut head = with_length("200 OK", DECISIVE);
    let tail = head.split_off(head.len() - 20);
    let ep = serve_once(Reply::Slow(head, Duration::from_millis(400), tail)).await;
    let t0 = std::time::Instant::now();
    assert_eq!(
        post(&ep, Duration::from_millis(100)).await,
        Err(PostError::Transport(TransportErr::Timeout))
    );
    assert!(
        t0.elapsed() < Duration::from_millis(350),
        "the deadline, not the server, ended it"
    );
}

#[tokio::test]
async fn jev_net_oversized_bodies_are_refused() {
    let big = http(
        "200 OK",
        &format!("Content-Length: {}\r\n", MAX_BODY + 1),
        b"{",
    );
    let ep = serve_once(Reply::Bytes(big)).await;
    assert_eq!(
        post(&ep, DEADLINE).await,
        Err(PostError::Malformed(MalformedKind::TooLarge))
    );
    let flood = http("200 OK", "", &vec![b' '; MAX_BODY + 10]);
    let ep = serve_once(Reply::Bytes(flood)).await;
    assert_eq!(
        post(&ep, DEADLINE).await,
        Err(PostError::Malformed(MalformedKind::TooLarge))
    );
}

#[tokio::test]
async fn jev_net_unresolvable_host() {
    // RFC 6761: `.invalid` never resolves.
    let ep = Endpoint {
        host: "jev-probe-test.invalid".into(),
        port: 8710,
    };
    assert_eq!(
        post(&ep, DEADLINE).await,
        Err(PostError::Transport(TransportErr::Resolve))
    );
}

#[test]
fn jev_net_endpoint_parsing() {
    assert_eq!(
        Endpoint::parse("cosmo.lan:8710"),
        Ok(Endpoint {
            host: "cosmo.lan".into(),
            port: 8710
        })
    );
    assert!(Endpoint::parse("cosmo.lan").is_err());
    assert!(Endpoint::parse(":8710").is_err());
    assert!(Endpoint::parse("h:99999").is_err());
}

// ── validation (pure) ───────────────────────────────────────────────────────────────────────

#[test]
fn jev_wire_captured_answers_gate_as_expected() {
    assert_eq!(
        val(DECISIVE, &binary_posture()),
        Ok(vec![QuestionResult::Choice(Decision::Apply(0))])
    );
    let unclear = val(UNCLEAR, &binary_posture()).expect("well-formed");
    assert!(matches!(
        unclear[0],
        QuestionResult::Choice(Decision::NoSignal {
            reason: NoSignalReason::Flat { .. },
            ..
        })
    ));
    let three = vec![QuestionSpec {
        key: QuestionKey::Posture,
        instructions: "Choose the engagement posture for this bot.",
        kind: QuestionKind::Choice(vec![
            ChoiceOption {
                label: "engage",
                desc: Phrase::lit("push the enemy"),
            },
            ChoiceOption {
                label: "trade",
                desc: Phrase::lit("hold position and trade shots"),
            },
            ChoiceOption {
                label: "disengage",
                desc: Phrase::lit("retreat and break line of sight"),
            },
        ]),
    }];
    let r = val(THREE_WAY, &three).expect("well-formed");
    assert!(matches!(
        r[0],
        QuestionResult::Choice(Decision::NoSignal {
            reason: NoSignalReason::Flat { .. },
            ..
        })
    ));
}

#[test]
fn jev_wire_structural_failures_reject_the_whole_response() {
    let asked = binary_posture();
    let ok_answer =
        r#"{"type":"choice","choice":"stay","probabilities":{"disengage":0.1,"stay":0.9}}"#;
    let cases: [(String, MalformedKind); 8] = [
        ("{not json".into(), MalformedKind::Json),
        (
            format!(r#"{{"model":"{MODEL}","answers":{{}}}}"#),
            MalformedKind::MissingKey,
        ),
        (
            format!(r#"{{"model":"{MODEL}"}}"#),
            MalformedKind::MissingKey,
        ),
        (
            format!(
                r#"{{"model":"{MODEL}","answers":{{"posture":{ok_answer},"region":{ok_answer}}}}}"#
            ),
            MalformedKind::ExtraKey,
        ),
        (
            format!(
                r#"{{"model":"{MODEL}","answers":{{"posture":{{"type":"choice","choice":"stay","probabilities":{{"disengage":0.1,"stay":0.8}}}}}}}}"#
            ),
            MalformedKind::BadProbabilities,
        ),
        (
            format!(r#"{{"model":"{MODEL}","answers":{{"posture":{{"type":"verdict","x":1}}}}}}"#),
            MalformedKind::UnknownType,
        ),
        (
            format!(r#"{{"model":"other/model","answers":{{"posture":{ok_answer}}}}}"#),
            MalformedKind::ModelMismatch,
        ),
        (
            format!(r#"{{"answers":{{"posture":{ok_answer}}}}}"#),
            MalformedKind::ModelMismatch,
        ),
    ];
    for (body, want) in cases {
        assert_eq!(val(&body, &asked), Err(want), "body: {body}");
    }
}

#[test]
fn jev_wire_label_not_offered_and_wrong_type_are_malformed() {
    let asked = binary_posture();
    let off = format!(
        r#"{{"model":"{MODEL}","answers":{{"posture":{{"type":"choice","choice":"engage","probabilities":{{"disengage":0.1,"stay":0.9}}}}}}}}"#
    );
    assert_eq!(val(&off, &asked), Err(MalformedKind::LabelNotOffered));
    let extra_label = format!(
        r#"{{"model":"{MODEL}","answers":{{"posture":{{"type":"choice","choice":"stay","probabilities":{{"disengage":0.1,"stay":0.8,"engage":0.1}}}}}}}}"#
    );
    assert_eq!(
        val(&extra_label, &asked),
        Err(MalformedKind::BadProbabilities)
    );
    let noul =
        format!(r#"{{"model":"{MODEL}","answers":{{"posture":{{"type":"noul","noul":0.4}}}}}}"#);
    assert_eq!(
        val(&noul, &asked),
        Err(MalformedKind::UnknownType),
        "asked choice, got noul"
    );
}

#[test]
fn jev_wire_unknown_extra_fields_are_ignored() {
    let body = format!(
        r#"{{"model":"{MODEL}","future":true,"answers":{{"posture":{{"type":"choice","choice":"stay","probabilities":{{"disengage":0.1,"stay":0.9}},"confidence":0.8,"rationale":"n/a"}}}},"usage":{{"input_tokens":7,"cached":3}}}}"#
    );
    let v = validate(
        body.as_bytes(),
        &binary_posture(),
        MODEL,
        &GateThresholds::default(),
    )
    .expect("valid");
    assert_eq!(v.results[0].1, QuestionResult::Choice(Decision::Apply(1)));
    assert_eq!(v.input_tokens, Some(7));
}

#[test]
fn jev_wire_score_and_noul_answers() {
    let asked = vec![
        QuestionSpec {
            key: QuestionKey::Objective,
            instructions: "How urgent is health?",
            kind: QuestionKind::Score(vec!["calm", "soon", "now"]),
        },
        QuestionSpec {
            key: QuestionKey::Posture,
            instructions: "Enough to decide?",
            kind: QuestionKind::Noul,
        },
    ];
    let body = format!(
        r#"{{"model":"{MODEL}","answers":{{"objective":{{"type":"score","score":1.9,"legend":{{"0":"calm"}},"probabilities":{{"0":0.05,"1":0.1,"2":0.85}}}},"posture":{{"type":"noul","noul":0.42}}}}}}"#
    );
    assert_eq!(
        val(&body, &asked),
        Ok(vec![
            QuestionResult::Score(Decision::Apply(2)),
            QuestionResult::Noul(0.42)
        ])
    );
}
