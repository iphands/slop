//! # brain::jev::decision — what an answer *is* (Plan 72 T3)
//!
//! [`Decision`] is the hub's classification of one question's answer, and it **is** the
//! observability design: every arm is one telemetry line and one counter. Its error payloads are
//! `Copy` enums (they become the `err=` field), never `String`s.

/// Summary statistics of one answer's probability vector (computed by [`super::gate`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistStats {
    /// Number of options.
    pub n: usize,
    /// Index of the most probable option.
    pub top: usize,
    /// Its probability.
    pub p1: f32,
    /// `p1 − p2` — the gate statistic.
    pub margin: f32,
    /// `exp(−Σ p ln p)` — effective number of options (logged; gates only n ≥ 4).
    pub n_eff: f32,
    /// `p1 − 1/n` — the cheap "said nothing at all" secondary (logged only).
    pub excess: f32,
}

/// Why an answer that parsed fine is still not a decision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoSignalReason {
    /// The distribution does not discriminate (margin below threshold, or `n_eff` too high for
    /// n ≥ 4). Both statistics are carried for the log line.
    Flat {
        /// `p1 − p2`.
        margin: f32,
        /// Effective number of options.
        n_eff: f32,
    },
    /// Fewer than two options — nothing to choose between.
    TooFewOptions {
        /// How many there were.
        n: usize,
    },
}

/// The response (or one answer in it) does not match the contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MalformedKind {
    /// The body is not the JSON we expect.
    Json,
    /// A requested key is missing from `answers`.
    MissingKey,
    /// `answers` has a key we did not ask for.
    ExtraKey,
    /// An answer `type` we do not know (never silently defaulted).
    UnknownType,
    /// Probabilities non-finite, negative, or not summing to 1 (±1e-3).
    BadProbabilities,
    /// `choice` names a label we did not offer.
    LabelNotOffered,
    /// The server reported a different model than configured.
    ModelMismatch,
    /// The response exceeded the size cap.
    TooLarge,
    /// HTTP framing could not be parsed (status line, headers, chunking, truncation).
    Framing,
}

impl MalformedKind {
    /// The `err=` slug.
    pub fn slug(self) -> &'static str {
        match self {
            MalformedKind::Json => "bad_json",
            MalformedKind::MissingKey => "missing_key",
            MalformedKind::ExtraKey => "extra_key",
            MalformedKind::UnknownType => "unknown_type",
            MalformedKind::BadProbabilities => "bad_probabilities",
            MalformedKind::LabelNotOffered => "label_not_offered",
            MalformedKind::ModelMismatch => "model_mismatch",
            MalformedKind::TooLarge => "too_large",
            MalformedKind::Framing => "framing",
        }
    }
}

/// The request never produced a usable response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportErr {
    /// Connection refused.
    Refused,
    /// The deadline (wrapping connect + write + read) expired.
    Timeout,
    /// The endpoint's name did not resolve.
    Resolve,
    /// The peer closed without sending a byte.
    Closed,
    /// A non-200 HTTP status.
    Http(u16),
    /// Any other socket error.
    Io,
}

impl TransportErr {
    /// The `err=` slug (`http_501`, `timeout`, …). Allocation-free except for `Http`.
    pub fn slug(self) -> std::borrow::Cow<'static, str> {
        match self {
            TransportErr::Refused => "refused".into(),
            TransportErr::Timeout => "timeout".into(),
            TransportErr::Resolve => "resolve".into(),
            TransportErr::Closed => "closed".into(),
            TransportErr::Http(code) => format!("http_{code}").into(),
            TransportErr::Io => "io".into(),
        }
    }
}

/// The hub's verdict on one question of one request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision<T> {
    /// A decision to apply.
    Apply(T),
    /// Parsed fine, but not a decision (see [`NoSignalReason`]).
    NoSignal {
        /// Why.
        reason: NoSignalReason,
        /// The distribution's statistics, for the log line.
        stats: DistStats,
    },
    /// The response broke the contract.
    Malformed(MalformedKind),
    /// No usable response.
    Transport(TransportErr),
    /// The circuit breaker is open; nothing was sent.
    CircuitOpen,
}
