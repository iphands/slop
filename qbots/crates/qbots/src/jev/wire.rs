//! # qbots::jev::wire — `/v1/systemone` response types and validation (Plan 72 T4)
//!
//! Contract (plan *The endpoint*):
//! ```text
//! → {"model":"…","answers":{key:{"type":"choice","choice":L,"probabilities":{label:p},"confidence":c}
//!                                | {"type":"score","score":f,"legend":{…},"probabilities":{…},"confidence":c}
//!                                | {"type":"noul","noul":p}},
//!    "usage":{"input_tokens":N,"output_tokens":0}}
//! ```
//! Rules: no `deny_unknown_fields` (the server is versioned independently — unknown fields are
//! ignored); an unknown answer `type` is `Malformed`, never defaulted; `answers` keys must
//! **exactly** equal the asked keys and any structural problem rejects the **whole** response
//! (partial application is where silent corruption lives); a missing or different `model` fails
//! closed. Well-formed answers are then gated (`brain::jev::gate`) into `Decision<usize>` —
//! the index into the **offered** order (labels never leave this module).

use std::collections::BTreeMap;

use brain::jev::decision::{Decision, MalformedKind};
use brain::jev::gate::{gate, GateThresholds};
use brain::jev::question::{QuestionKey, QuestionKind, QuestionSpec};
use serde::Deserialize;

/// The top-level response.
#[derive(Debug, Deserialize)]
pub struct Response {
    /// The serving model's slug (advisory in the type, asserted by [`validate`]).
    #[serde(default)]
    pub model: Option<String>,
    /// One answer per asked key.
    #[serde(default)]
    pub answers: Option<BTreeMap<String, Answer>>,
    /// Token accounting (advisory).
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// Token accounting.
#[derive(Debug, Default, Deserialize)]
pub struct Usage {
    /// The server's count of input tokens — fed to the hub's bucket (true-up).
    #[serde(default)]
    pub input_tokens: Option<u32>,
}

/// One answer, tagged by `type`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    /// A `choice` answer.
    Choice {
        /// The chosen label (required).
        choice: String,
        /// Label → probability (required).
        probabilities: BTreeMap<String, f32>,
    },
    /// A `score` answer.
    Score {
        /// The float score — required by the contract (a body without it is `Malformed`), but
        /// its value is not used: the level is the argmax of `probabilities`.
        #[serde(rename = "score")]
        _score: f32,
        /// Level index (`"0"`, `"1"`, …) → probability (required).
        probabilities: BTreeMap<String, f32>,
    },
    /// A `noul` answer.
    Noul {
        /// The probability (required).
        noul: f32,
    },
    /// Any `type` we do not know.
    #[serde(other)]
    Unknown,
}

/// One question's validated, gated result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuestionResult {
    /// `choice`: `Apply(index into the offered options)` or `NoSignal`.
    Choice(Decision<usize>),
    /// `score`: `Apply(level index)` (argmax of the level probabilities) or `NoSignal`.
    Score(Decision<usize>),
    /// `noul`: the raw probability (a canary — never gated, never load-bearing).
    Noul(f32),
}

/// A response that passed every structural check.
#[derive(Clone, Debug, PartialEq)]
pub struct Validated {
    /// Results in the **asked** order.
    pub results: Vec<(QuestionKey, QuestionResult)>,
    /// `usage.input_tokens`, if reported.
    pub input_tokens: Option<u32>,
}

/// Parse, check against what was asked and the configured model, and gate each answer.
pub fn validate(
    body: &[u8],
    asked: &[QuestionSpec],
    expected_model: &str,
    th: &GateThresholds,
) -> Result<Validated, MalformedKind> {
    let resp: Response = serde_json::from_slice(body).map_err(|_| MalformedKind::Json)?;
    if resp.model.as_deref() != Some(expected_model) {
        return Err(MalformedKind::ModelMismatch);
    }
    let mut answers = resp.answers.unwrap_or_default();
    let mut results = Vec::with_capacity(asked.len());
    for q in asked {
        let a = answers
            .remove(q.key.wire_key())
            .ok_or(MalformedKind::MissingKey)?;
        results.push((q.key, check(&q.kind, a, th)?));
    }
    if !answers.is_empty() {
        return Err(MalformedKind::ExtraKey);
    }
    Ok(Validated {
        results,
        input_tokens: resp.usage.and_then(|u| u.input_tokens),
    })
}

/// Check one answer against its question and gate it.
fn check(
    kind: &QuestionKind,
    a: Answer,
    th: &GateThresholds,
) -> Result<QuestionResult, MalformedKind> {
    match (kind, a) {
        (_, Answer::Unknown) => Err(MalformedKind::UnknownType),
        (
            QuestionKind::Choice(opts),
            Answer::Choice {
                choice,
                probabilities,
            },
        ) => {
            if !opts.iter().any(|o| o.label == choice) {
                return Err(MalformedKind::LabelNotOffered);
            }
            let labels: Vec<&str> = opts.iter().map(|o| o.label).collect();
            let p = ordered(&labels, probabilities)?;
            Ok(QuestionResult::Choice(gated(&p, th)?))
        }
        (QuestionKind::Score(levels), Answer::Score { probabilities, .. }) => {
            let names: Vec<String> = (0..levels.len()).map(|i| i.to_string()).collect();
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let p = ordered(&names, probabilities)?;
            Ok(QuestionResult::Score(gated(&p, th)?))
        }
        (QuestionKind::Noul, Answer::Noul { noul }) => {
            if !(0.0..=1.0).contains(&noul) {
                return Err(MalformedKind::BadProbabilities);
            }
            Ok(QuestionResult::Noul(noul))
        }
        // A known type, but not the one we asked for.
        _ => Err(MalformedKind::UnknownType),
    }
}

/// The probability vector in `keys` order; the map's keys must be exactly `keys`.
fn ordered(keys: &[&str], mut p: BTreeMap<String, f32>) -> Result<Vec<f32>, MalformedKind> {
    let v = keys
        .iter()
        .map(|k| p.remove(*k).ok_or(MalformedKind::BadProbabilities))
        .collect::<Result<Vec<f32>, _>>()?;
    if p.is_empty() {
        Ok(v)
    } else {
        Err(MalformedKind::BadProbabilities)
    }
}

/// Gate, turning an ill-formed vector into a whole-response rejection.
fn gated(p: &[f32], th: &GateThresholds) -> Result<Decision<usize>, MalformedKind> {
    match gate(p, th) {
        Decision::Malformed(k) => Err(k),
        d => Ok(d),
    }
}
