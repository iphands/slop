//! # qbots::jev::render — the exhaustive state/request renderer (Plan 72 T2)
//!
//! **This is the prompt's security boundary and its determinism boundary.**
//!
//! - Every byte written comes from a `&'static str`, an integer, or a closed enum's slug. The
//!   match over [`Field`] is exhaustive, so adding a state field is a **compile error here**
//!   until someone reviews what the prompt now says.
//! - The model is deterministic, so serialization order is *behaviour*: lines are written in
//!   [`FieldKey`] order by hand, and the JSON request is written by hand in declared order
//!   (`serde_json`'s `Map` sorts keys alphabetically in this build — it would silently reorder
//!   the offered options). `serde_json` is used only to escape strings.
//! - A change to any rendered byte must bump `brain::jev::state::STATE_SCHEMA_VERSION` and
//!   re-run the live matrix; the golden tests below fail with that instruction.

use std::fmt::Write as _;

use brain::jev::question::{Piece, QuestionKind, QuestionSpec};
use brain::jev::state::{weapon_slug, DecisionState, DistBand, Field, FieldKey, MAX_COUNT};

/// A rendered state, plus what was dropped to fit the budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedState {
    /// The state text, one line per field.
    pub text: String,
    /// Fields dropped to fit `max_tokens`, lowest keep-priority first. Logged by the caller.
    pub dropped: Vec<FieldKey>,
    /// Estimated tokens of `text` ([`est_tokens`]).
    pub est_tokens: usize,
}

/// The token estimate used for budgeting *before* a request (the hub trues it up with the
/// server's `usage.input_tokens` afterwards): `ceil(chars / 4)`.
pub fn est_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// Render one field as one line: `<tag padded to 5> <facts>`.
pub fn render_field(f: &Field) -> String {
    let mut s = format!("{:<5} ", f.key().tag());
    match f {
        Field::Self_(x) => {
            let held = x.held.map_or("unknown", weapon_slug);
            let _ = write!(
                s,
                "hp:{}({}) armor:{} held:{} ammo:{} low_ammo:{}",
                x.hp,
                x.band.slug(),
                x.armor,
                held,
                x.ammo,
                yes_no(x.ammo_low)
            );
        }
        Field::Dmg(x) => {
            let since = x.since.map_or("never", |b| b.slug());
            let _ = write!(s, "recent:{} since:{}", x.recent, since);
        }
        Field::Enemy(x) => {
            let _ = write!(
                s,
                "visible:{}{} nearest:{} moving:{}",
                x.count,
                plus(x.count),
                band(x.nearest),
                yes_no(x.moving)
            );
        }
        Field::Loc(x) => {
            let exits = x.exits.map(|n| format!("{n}{}", plus(n)));
            let _ = write!(
                s,
                "exits:{} moving:{}",
                exits.as_deref().unwrap_or("unknown"),
                yes_no(x.moving)
            );
        }
        Field::Items(x) => {
            let _ = write!(
                s,
                "health:{} armor:{} weapon:{} powerup:{}",
                band(x.health),
                band(x.armor),
                band(x.weapon),
                band(x.powerup)
            );
        }
        Field::Map(x) => {
            let _ = write!(s, "{} wants:{}", x.map.slug(), x.gap.slug());
        }
    }
    s
}

/// Render the whole state, dropping the lowest keep-priority fields (never `Self_`) until it
/// fits `max_tokens`. Lines are joined with `\n`, no trailing newline.
pub fn render_state(state: &DecisionState, max_tokens: usize) -> RenderedState {
    let mut s = state.clone();
    let mut dropped = Vec::new();
    loop {
        let text = s
            .fields()
            .iter()
            .map(render_field)
            .collect::<Vec<_>>()
            .join("\n");
        let est = est_tokens(&text);
        let victim = s
            .fields()
            .iter()
            .map(Field::key)
            .filter(|k| *k != FieldKey::Self_)
            .min_by_key(|k| k.keep_priority());
        match victim {
            Some(k) if est > max_tokens => {
                dropped.push(k);
                s = s.without(k);
            }
            _ => {
                return RenderedState {
                    text,
                    dropped,
                    est_tokens: est,
                }
            }
        }
    }
}

/// The full `/v1/systemone` request body, written in declared order:
/// `{"state":…,"questions":{<key>:{"type":…,"instructions":…,"criteria":…},…}}`.
/// Questions keep the order given; choice options keep their offered order.
pub fn render_request(state: &RenderedState, questions: &[QuestionSpec]) -> String {
    let mut out = String::from("{\"state\":");
    out.push_str(&json_str(&state.text));
    out.push_str(",\"questions\":{");
    for (i, q) in questions.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_str(q.key.wire_key()));
        out.push_str(":{\"type\":");
        let ty = match q.kind {
            QuestionKind::Choice(_) => "choice",
            QuestionKind::Score(_) => "score",
            QuestionKind::Noul => "noul",
        };
        out.push_str(&json_str(ty));
        out.push_str(",\"instructions\":");
        out.push_str(&json_str(q.instructions));
        match &q.kind {
            QuestionKind::Choice(opts) => {
                out.push_str(",\"criteria\":{");
                for (j, o) in opts.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    out.push_str(&json_str(o.label));
                    out.push(':');
                    out.push_str(&json_str(&phrase(&o.desc.0)));
                }
                out.push('}');
            }
            QuestionKind::Score(levels) => {
                out.push_str(",\"criteria\":[");
                for (j, l) in levels.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    out.push_str(&json_str(l));
                }
                out.push(']');
            }
            QuestionKind::Noul => {}
        }
        out.push('}');
    }
    out.push_str("}}");
    out
}

/// Join closed pieces.
fn phrase(pieces: &[Piece]) -> String {
    let mut s = String::new();
    for p in pieces {
        match p {
            Piece::Lit(t) => s.push_str(t),
            Piece::Num(n) => {
                let _ = write!(s, "{n}");
            }
        }
    }
    s
}

/// A JSON string literal (quotes + escapes) — the only thing `serde_json` does here.
fn json_str(s: &str) -> String {
    serde_json::to_string(s).expect("serializing a &str to JSON cannot fail")
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

/// `"+"` when a count hit the cap (`9+` = nine or more).
fn plus(n: u32) -> &'static str {
    if n >= MAX_COUNT {
        "+"
    } else {
        ""
    }
}

fn band(b: Option<DistBand>) -> &'static str {
    b.map_or("none", DistBand::slug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain::jev::question::{ChoiceOption, Phrase, QuestionKey};
    use brain::jev::state::{
        DmgFacts, EnemyFacts, HealthBand, ItemFacts, LoadoutGap, LocFacts, MapFacts, MapId,
        SelfFacts, SinceBand, STATE_SCHEMA_VERSION,
    };
    use brain::weapons::Weapon;

    const CHANGED: &str = "state serialization changed — this changes model behaviour; bump \
                           STATE_SCHEMA_VERSION and re-run the live matrix";

    /// The named golden state: a hurt bot holding a railgun, two enemies near, armor in reach.
    fn golden_state() -> DecisionState {
        DecisionState::from_fields(vec![
            Field::Self_(SelfFacts {
                hp: 38,
                band: HealthBand::Lo,
                armor: 12,
                held: Some(Weapon::Railgun),
                ammo: 3,
                ammo_low: true,
            }),
            Field::Dmg(DmgFacts {
                recent: 74,
                since: Some(SinceBand::Recent),
            }),
            Field::Enemy(EnemyFacts {
                count: 2,
                nearest: Some(DistBand::Near),
                moving: true,
            }),
            Field::Loc(LocFacts {
                exits: Some(2),
                moving: false,
            }),
            Field::Items(ItemFacts {
                health: None,
                armor: Some(DistBand::Touch),
                weapon: None,
                powerup: Some(DistBand::Mid),
            }),
            Field::Map(MapFacts {
                map: MapId::Q2dm1,
                gap: LoadoutGap::Health,
            }),
        ])
    }

    const GOLDEN_STATE: &str = "self  hp:38(lo) armor:12 held:railgun ammo:3 low_ammo:yes\n\
                                dmg   recent:74 since:recent\n\
                                enemy visible:2 nearest:near moving:yes\n\
                                loc   exits:2 moving:no\n\
                                items health:none armor:touch weapon:none powerup:mid\n\
                                map   q2dm1 wants:health";

    #[test]
    fn jev_render_golden_state_is_byte_exact() {
        assert_eq!(STATE_SCHEMA_VERSION, 1, "{CHANGED}");
        let r = render_state(&golden_state(), 512);
        assert_eq!(r.text, GOLDEN_STATE, "{CHANGED}");
        assert!(r.dropped.is_empty());
        assert_eq!(r.est_tokens, est_tokens(GOLDEN_STATE));
    }

    #[test]
    fn jev_render_is_deterministic_over_100_renders() {
        let s = golden_state();
        let first = render_state(&s, 512);
        for _ in 0..100 {
            assert_eq!(render_state(&s, 512), first);
        }
    }

    #[test]
    fn jev_render_over_budget_drops_lowest_priority_first_and_names_it() {
        let full = render_state(&golden_state(), 512);
        // One token under the full size: exactly the lowest keep-priority field (Loc) goes.
        let r = render_state(&golden_state(), full.est_tokens - 1);
        assert_eq!(r.dropped, vec![FieldKey::Loc]);
        assert!(!r.text.contains("\nloc "));
        assert!(r.est_tokens < full.est_tokens);
        // A tiny budget drops everything but Self_, in keep-priority order, and stops there.
        let r = render_state(&golden_state(), 1);
        assert_eq!(
            r.dropped,
            vec![
                FieldKey::Loc,
                FieldKey::Map,
                FieldKey::Items,
                FieldKey::Dmg,
                FieldKey::Enemy
            ]
        );
        assert!(r.text.starts_with("self "));
        assert_eq!(r.text.lines().count(), 1);
    }

    fn golden_questions() -> Vec<QuestionSpec> {
        vec![
            QuestionSpec {
                key: QuestionKey::Objective,
                instructions: "Choose the bot's next objective.",
                kind: QuestionKind::Choice(vec![
                    ChoiceOption {
                        label: "seek_armor",
                        desc: Phrase(vec![
                            Piece::Lit("take the body armor "),
                            Piece::Num(1),
                            Piece::Lit(" step away"),
                        ]),
                    },
                    ChoiceOption {
                        label: "avoid_fight",
                        desc: Phrase::lit("break line of sight with the \"two\" enemies"),
                    },
                ]),
            },
            QuestionSpec {
                key: QuestionKey::Posture,
                instructions: "Is there enough here to decide?",
                kind: QuestionKind::Noul,
            },
        ]
    }

    #[test]
    fn jev_render_request_golden_keeps_declared_order_and_escapes() {
        let r = render_state(&golden_state(), 512);
        let body = render_request(&r, &golden_questions());
        let want = concat!(
            "{\"state\":\"self  hp:38(lo) armor:12 held:railgun ammo:3 low_ammo:yes\\n",
            "dmg   recent:74 since:recent\\nenemy visible:2 nearest:near moving:yes\\n",
            "loc   exits:2 moving:no\\nitems health:none armor:touch weapon:none powerup:mid\\n",
            "map   q2dm1 wants:health\",\"questions\":{",
            "\"objective\":{\"type\":\"choice\",\"instructions\":\"Choose the bot's next objective.\",",
            "\"criteria\":{\"seek_armor\":\"take the body armor 1 step away\",",
            "\"avoid_fight\":\"break line of sight with the \\\"two\\\" enemies\"}},",
            "\"posture\":{\"type\":\"noul\",\"instructions\":\"Is there enough here to decide?\"}}}"
        );
        assert_eq!(body, want, "{CHANGED}");
        // It is valid JSON, and the offered order survived (serde_json's Map would sort it).
        let v: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        assert_eq!(v["questions"]["objective"]["type"], "choice");
        assert!(body.find("seek_armor") < body.find("avoid_fight"));
    }

    #[test]
    fn jev_render_score_question_renders_levels_as_an_array() {
        let q = QuestionSpec {
            key: QuestionKey::Posture,
            instructions: "How dangerous is staying?",
            kind: QuestionKind::Score(vec!["safe", "risky", "lethal"]),
        };
        let r = render_state(&golden_state(), 512);
        let body = render_request(&r, &[q]);
        let v: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        assert_eq!(
            v["questions"]["posture"]["criteria"],
            serde_json::json!(["safe", "risky", "lethal"])
        );
    }
}
