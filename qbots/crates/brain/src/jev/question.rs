//! # brain::jev::question — closed question specs (Plan 72 T2)
//!
//! A [`QuestionSpec`] is one entry of a `/v1/systemone` request's `questions` object. Every piece
//! of text it can carry is a `&'static str` (a literal in our source) or a number, so a question
//! — like the state — cannot carry wire text. Criteria descriptions that need facts (a distance
//! band, a count) are built from [`Piece`]s rather than formatted `String`s, for the same reason.

/// The question keys we ask. Region keys arrive with T14.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QuestionKey {
    /// Which objective to pursue (T10).
    Objective,
    /// Whether to disengage (T12).
    Posture,
}

impl QuestionKey {
    /// The key in the request's `questions` object (and the response's `answers`).
    pub fn wire_key(self) -> &'static str {
        match self {
            QuestionKey::Objective => "objective",
            QuestionKey::Posture => "posture",
        }
    }
}

/// One closed piece of a criteria description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    /// Literal text from our source.
    Lit(&'static str),
    /// A number (rendered in decimal).
    Num(u32),
}

/// A criteria description: closed pieces, joined without separators by the renderer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Phrase(pub Vec<Piece>);

impl Phrase {
    /// A phrase of one literal.
    pub fn lit(s: &'static str) -> Self {
        Phrase(vec![Piece::Lit(s)])
    }
}

/// One option of a `choice` question: a label (what comes back in `choice`) and a description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceOption {
    /// The label — a move slug, never an index or a wire string.
    pub label: &'static str,
    /// What the move means, in closed pieces.
    pub desc: Phrase,
}

/// The three question types the endpoint accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuestionKind {
    /// Pick one labelled option (offered order is preserved on the wire).
    Choice(Vec<ChoiceOption>),
    /// Grade on ordered levels `0..n`.
    Score(Vec<&'static str>),
    /// "Is there enough here to answer?" — a single probability.
    Noul,
}

/// One question of a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestionSpec {
    /// Its key.
    pub key: QuestionKey,
    /// The instruction text.
    pub instructions: &'static str,
    /// Its type and options.
    pub kind: QuestionKind,
}

/// The posture question as the language lab measured it (Plan 73, round 4). `hold` is the no-op.
pub fn posture_question() -> QuestionSpec {
    QuestionSpec {
        key: QuestionKey::Posture,
        instructions: "Should this bot break off the current fight to survive, or leave the fight \
                       to its own judgement?",
        kind: QuestionKind::Choice(vec![
            ChoiceOption {
                label: DISENGAGE,
                desc: Phrase::lit("retreat now and break line of sight"),
            },
            ChoiceOption {
                label: HOLD,
                desc: Phrase::lit("keep fighting as the bot sees fit"),
            },
        ]),
    }
}

/// The objective question as the language lab measured it (Plan 73, round 4). `carry_on` is
/// the no-op ("keep roaming").
pub fn objective_question() -> QuestionSpec {
    QuestionSpec {
        key: QuestionKey::Objective,
        instructions: "Choose this bot's next objective.",
        kind: QuestionKind::Choice(vec![
            ChoiceOption {
                label: SEEK_HEALTH,
                desc: Phrase::lit("go to the nearest health pickup"),
            },
            ChoiceOption {
                label: SEEK_ARMOR,
                desc: Phrase::lit("go to the nearest armor pickup"),
            },
            ChoiceOption {
                label: CARRY_ON,
                desc: Phrase::lit("no particular objective: keep roaming as usual"),
            },
        ]),
    }
}

/// Posture option: retreat and do not chase.
pub const DISENGAGE: &str = "disengage";
/// Posture option (no-op): leave the fight to q3.
pub const HOLD: &str = "hold";
/// Objective option: walk to the nearest health pickup.
pub const SEEK_HEALTH: &str = "seek_health";
/// Objective option: walk to the nearest armor pickup.
pub const SEEK_ARMOR: &str = "seek_armor";
/// Objective option (no-op): keep roaming.
pub const CARRY_ON: &str = "carry_on";
