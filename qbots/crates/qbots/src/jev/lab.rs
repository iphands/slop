//! # qbots::jev::lab — the language lab (Plan 73 T2)
//!
//! The decision model is frozen; **we** learn which facts and wording make it answer well. The
//! lab asks the same human-labelled situations in different **variants** — each a closed,
//! reviewed code transform changing exactly one thing — and scores them.
//!
//! - **Closed variants.** A variant is a pure function of the rendered state text and the
//!   question spec. It never reads a file or takes free text: every word it can add is a
//!   `&'static str` in this file, so Plan 72's prompt security boundary holds.
//! - **Tuning vs holdout.** A case is in the holdout iff the FNV-1a hash of its file name is odd
//!   — content-independent, stable, and decided before anyone sees an answer. Variants are
//!   compared on the tuning half; the holdout is scored once, for the final variant.
//! - **Metrics** (plan *Lab metrics*): right-and-confident, confident-wrong, accuracy, mean
//!   margin, ambiguous leakage, flat max margin, separation.

use std::path::{Path, PathBuf};

use brain::jev::question::{ChoiceOption, Phrase, QuestionKey, QuestionKind, QuestionSpec};

/// The labelled role of a corpus case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaseKind {
    /// Any decent player answers this instantly — with this label.
    Obvious(&'static str),
    /// Reasonable players would disagree.
    Ambiguous,
    /// No information relevant to the question.
    Flat,
}

/// One corpus file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusCase {
    /// File name (decides the split).
    pub name: String,
    /// The state text (rendered-state format).
    pub text: String,
    /// Its label.
    pub kind: CaseKind,
}

/// Which half of the corpus.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Half {
    /// Variants are compared here.
    Tuning,
    /// Scored once, for the final variant.
    Holdout,
}

/// FNV-1a 64 — the split's hash (no dependency, stable across platforms and releases).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The half a case belongs to: holdout iff its file name's FNV-1a hash is odd.
pub fn half_of(name: &str) -> Half {
    if fnv1a(name.as_bytes()) % 2 == 1 {
        Half::Holdout
    } else {
        Half::Tuning
    }
}

/// Refuse a holdout run unless explicitly confirmed (the holdout is spent once per wording).
pub fn guard_half(half: Half, confirm_holdout: bool) -> Result<(), String> {
    if half == Half::Holdout && !confirm_holdout {
        return Err("the holdout is scored ONCE, for the final variant — pass \
                    --confirm-holdout and record the use in the Plan 73 tracker"
            .into());
    }
    Ok(())
}

/// Load `<qdir>/obvious/<label>/*.txt`, `<qdir>/ambiguous/*.txt`, `<qdir>/flat/*.txt`. Every
/// obvious sub-directory must name an option of `q`.
pub fn load_corpus(qdir: &Path, q: &QuestionSpec) -> Result<Vec<CorpusCase>, String> {
    let QuestionKind::Choice(opts) = &q.kind else {
        return Err("the lab needs a choice question".into());
    };
    let mut out = Vec::new();
    for sub in sorted(&qdir.join("obvious"), |p| p.is_dir())? {
        let name = sub.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let label = opts
            .iter()
            .map(|o| o.label)
            .find(|l| *l == name)
            .ok_or_else(|| format!("{}: '{name}' is not an option", sub.display()))?;
        out.extend(read_cases(&sub, CaseKind::Obvious(label))?);
    }
    out.extend(read_cases(&qdir.join("ambiguous"), CaseKind::Ambiguous)?);
    out.extend(read_cases(&qdir.join("flat"), CaseKind::Flat)?);
    Ok(out)
}

fn sorted(dir: &Path, keep: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>, String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| keep(p))
        .collect();
    v.sort();
    Ok(v)
}

fn read_cases(dir: &Path, kind: CaseKind) -> Result<Vec<CorpusCase>, String> {
    sorted(dir, |p| p.extension().is_some_and(|x| x == "txt"))?
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            Ok(CorpusCase {
                name: p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_string(),
                text: text.trim_end().to_string(),
                kind: kind.clone(),
            })
        })
        .collect()
}

/// Counts per half and kind: `(obvious, ambiguous, flat)`.
pub fn split_counts(cases: &[CorpusCase], half: Half) -> (usize, usize, usize) {
    let mut c = (0, 0, 0);
    for k in cases.iter().filter(|k| half_of(&k.name) == half) {
        match k.kind {
            CaseKind::Obvious(_) => c.0 += 1,
            CaseKind::Ambiguous => c.1 += 1,
            CaseKind::Flat => c.2 += 1,
        }
    }
    c
}

// ── variants ────────────────────────────────────────────────────────────────────────────

/// One closed change to the baseline. Families: facts (drop/add one line), form (how a fact
/// is written), options (descriptions), instructions (the question sentence).
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Variant {
    /// No change — the production rendering and the probe's catalogue question.
    Baseline,
    /// facts: drop the `dmg` line.
    DropDmg,
    /// facts: drop the `loc` line.
    DropLoc,
    /// facts: drop the `map` line (incl. `wants:`).
    DropMap,
    /// facts: add a derived `threat:` line (enemies × distance × our condition).
    AddThreat,
    /// form: `hp:15(crit)` → `hp:crit` (band only, no number).
    HpBandOnly,
    /// form: append one plain-English sentence summarising the situation.
    AddSummary,
    /// options: descriptions state each option's consequence.
    OptionsConsequence,
    /// instructions: state the decision criterion explicitly.
    InstructionCriterion,
    /// instructions: frame the model as a deathmatch coach.
    InstructionRole,
}

impl Variant {
    /// Every variant, baseline first.
    pub const ALL: [Variant; 10] = [
        Variant::Baseline,
        Variant::DropDmg,
        Variant::DropLoc,
        Variant::DropMap,
        Variant::AddThreat,
        Variant::HpBandOnly,
        Variant::AddSummary,
        Variant::OptionsConsequence,
        Variant::InstructionCriterion,
        Variant::InstructionRole,
    ];

    /// Apply to a state text and a question. Pure and deterministic.
    pub fn apply(self, text: &str, q: &QuestionSpec) -> (String, QuestionSpec) {
        let mut q = q.clone();
        let text = match self {
            Variant::Baseline => text.to_string(),
            Variant::DropDmg => drop_line(text, "dmg"),
            Variant::DropLoc => drop_line(text, "loc"),
            Variant::DropMap => drop_line(text, "map"),
            Variant::AddThreat => append(text, &format!("threat {}", threat(&Facts::parse(text)))),
            Variant::HpBandOnly => hp_band_only(text),
            Variant::AddSummary => append(text, summary(&Facts::parse(text))),
            Variant::OptionsConsequence => {
                q.kind = consequence_options(q.key, &q.kind);
                text.to_string()
            }
            Variant::InstructionCriterion => {
                q.instructions = criterion_instruction(q.key);
                text.to_string()
            }
            Variant::InstructionRole => {
                q.instructions = role_instruction(q.key);
                text.to_string()
            }
        };
        (text, q)
    }
}

fn drop_line(text: &str, tag: &str) -> String {
    text.lines()
        .filter(|l| l.split_whitespace().next() != Some(tag))
        .collect::<Vec<_>>()
        .join("\n")
}

fn append(text: &str, line: &str) -> String {
    if text.is_empty() {
        line.to_string()
    } else {
        format!("{text}\n{line}")
    }
}

/// `hp:<n>(<band>)` → `hp:<band>` on the `self` line; everything else unchanged.
fn hp_band_only(text: &str) -> String {
    text.lines()
        .map(|l| {
            if l.split_whitespace().next() != Some("self") {
                return l.to_string();
            }
            l.split(' ')
                .map(
                    |tok| match tok.strip_prefix("hp:").and_then(|r| r.split_once('(')) {
                        Some((_, band)) => format!("hp:{}", band.trim_end_matches(')')),
                        None => tok.to_string(),
                    },
                )
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The facts a derived line needs, read back from the closed rendering (missing ⇒ unknown).
#[derive(Debug, Default, PartialEq)]
struct Facts {
    band: Option<String>,
    armor: Option<u32>,
    weak_weapon: Option<bool>,
    enemies: Option<u32>,
    nearest: Option<String>,
}

impl Facts {
    fn parse(text: &str) -> Self {
        let mut f = Facts::default();
        for tok in text.split_whitespace() {
            let Some((k, v)) = tok.split_once(':') else {
                continue;
            };
            match k {
                "hp" => {
                    f.band = v
                        .split_once('(')
                        .map(|(_, b)| b.trim_end_matches(')').into())
                }
                "armor" if f.armor.is_none() => f.armor = v.parse().ok(),
                "held" => f.weak_weapon = Some(matches!(v, "blaster" | "shotgun" | "machinegun")),
                "visible" => f.enemies = v.trim_end_matches('+').parse().ok(),
                "nearest" => f.nearest = Some(v.into()),
                _ => {}
            }
        }
        f
    }
}

/// `none | low | medium | high` from enemies × distance × our condition.
fn threat(f: &Facts) -> &'static str {
    let Some(n) = f.enemies.filter(|&n| n > 0) else {
        return "none";
    };
    let close = match f.nearest.as_deref() {
        Some("touch") => 3,
        Some("near") => 2,
        Some("mid") => 1,
        _ => 0,
    };
    let frail = match f.band.as_deref() {
        Some("crit") => 3,
        Some("lo") => 2,
        Some("mid") => 1,
        _ => 0,
    } + u32::from(f.weak_weapon.unwrap_or(false))
        + u32::from(f.armor.unwrap_or(0) < 25);
    match n.min(4) * 2 + close + frail {
        0..=4 => "low",
        5..=8 => "medium",
        _ => "high",
    }
}

/// One closed sentence — every word is a literal here.
fn summary(f: &Facts) -> &'static str {
    let hurt = matches!(f.band.as_deref(), Some("crit" | "lo"));
    let weak = f.weak_weapon.unwrap_or(false);
    let enemies = f.enemies.unwrap_or(0);
    let close = matches!(f.nearest.as_deref(), Some("touch" | "near"));
    match (hurt, weak, enemies > 0, close) {
        (true, true, true, true) => "summary: badly hurt, weak weapon, enemies close",
        (true, _, true, true) => "summary: badly hurt with enemies close",
        (true, _, true, false) => "summary: badly hurt, enemies at a distance",
        (true, _, false, _) => "summary: badly hurt, no enemy in sight",
        (false, true, true, _) => "summary: healthy but only a weak weapon, enemies in sight",
        (false, false, true, true) => "summary: healthy and well armed, enemies close",
        (false, false, true, false) => "summary: healthy and well armed, enemies at a distance",
        (false, _, false, _) => "summary: healthy, no enemy in sight",
    }
}

fn consequence_options(key: QuestionKey, kind: &QuestionKind) -> QuestionKind {
    let QuestionKind::Choice(opts) = kind else {
        return kind.clone();
    };
    let desc = |label: &str| -> Option<&'static str> {
        Some(match (key, label) {
            (QuestionKey::Posture, "disengage") => {
                "leave the fight now: the bot survives but gives up the kill"
            }
            (QuestionKey::Posture, "hold") => {
                "keep fighting: wins the kill if stronger, dies if outmatched"
            }
            (QuestionKey::Objective, "seek_health") => {
                "walk to the health pickup: more hit points, time away from the action"
            }
            (QuestionKey::Objective, "seek_armor") => {
                "walk to the armor pickup: absorbs future damage, time away from the action"
            }
            (QuestionKey::Objective, "hold_position") => {
                "stay here: no pickup, but the bot keeps control of this area"
            }
            _ => return None,
        })
    };
    QuestionKind::Choice(
        opts.iter()
            .map(|o| ChoiceOption {
                label: o.label,
                desc: desc(o.label).map_or_else(|| o.desc.clone(), Phrase::lit),
            })
            .collect(),
    )
}

fn criterion_instruction(key: QuestionKey) -> &'static str {
    match key {
        QuestionKey::Posture => {
            "Choose disengage only if staying in this fight would likely get the bot killed; \
             otherwise choose hold."
        }
        QuestionKey::Objective => {
            "Choose the pickup the bot needs most right now; choose hold_position only if it \
             needs nothing that is in reach."
        }
    }
}

fn role_instruction(key: QuestionKey) -> &'static str {
    match key {
        QuestionKey::Posture => {
            "You are an expert Quake 2 deathmatch coach. Should this player break off the \
             current fight?"
        }
        QuestionKey::Objective => {
            "You are an expert Quake 2 deathmatch coach. What should this player do next?"
        }
    }
}

// ── scoring ─────────────────────────────────────────────────────────────────────────────

/// One scored answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Scored {
    /// The case's label.
    pub kind: CaseKind,
    /// The model's top choice.
    pub top: &'static str,
    /// `p1 − p2`.
    pub margin: f32,
}

/// The lab metrics for one variant on one half (plan *Lab metrics*).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metrics {
    /// Obvious cases scored.
    pub obvious: usize,
    /// Share of obvious cases whose top choice is the expected label.
    pub accuracy: f32,
    /// Share right **and** above the threshold — the number to raise.
    pub right_confident: f32,
    /// Share wrong **and** above the threshold — must stay ~0.
    pub confident_wrong: f32,
    /// Mean margin over correct obvious answers.
    pub margin_correct: f32,
    /// Share of ambiguous cases above the threshold.
    pub ambiguous_leak: f32,
    /// The most confident answer to a flat case.
    pub flat_max: f32,
    /// `margin_correct` − mean ambiguous margin.
    pub separation: f32,
    /// Share of ambiguous cases above the threshold **with an acting answer** (not the
    /// question's no-op label) — the leakage that would change play.
    pub ambiguous_leak_acting: f32,
    /// The most confident **acting** answer to a flat case — the costly kind of flat answer.
    pub flat_max_acting: f32,
}

/// Compute [`Metrics`] at threshold `t`. `noop` is the question's do-nothing label (posture's
/// `hold` — "leave it to q3"): a confident no-op answer on an ambiguous or flat case changes
/// nothing, so the `_acting` metrics exclude it.
pub fn score(answers: &[Scored], t: f32, noop: Option<&str>) -> Metrics {
    let acting = |a: &Scored| noop != Some(a.top);
    let share = |n: usize, of: usize| if of == 0 { 0.0 } else { n as f32 / of as f32 };
    let mean = |v: &[f32]| {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f32>() / v.len() as f32
        }
    };
    let obvious: Vec<(bool, f32)> = answers
        .iter()
        .filter_map(|a| match a.kind {
            CaseKind::Obvious(want) => Some((a.top == want, a.margin)),
            _ => None,
        })
        .collect();
    let amb: Vec<f32> = answers
        .iter()
        .filter(|a| a.kind == CaseKind::Ambiguous)
        .map(|a| a.margin)
        .collect();
    let correct: Vec<f32> = obvious.iter().filter(|o| o.0).map(|o| o.1).collect();
    let n = obvious.len();
    let amb_acting = answers
        .iter()
        .filter(|a| a.kind == CaseKind::Ambiguous && a.margin >= t && acting(a))
        .count();
    let flat_max_acting = answers
        .iter()
        .filter(|a| a.kind == CaseKind::Flat && acting(a))
        .map(|a| a.margin)
        .fold(0.0, f32::max);
    let margin_correct = mean(&correct);
    Metrics {
        obvious: n,
        accuracy: share(correct.len(), n),
        right_confident: share(obvious.iter().filter(|o| o.0 && o.1 >= t).count(), n),
        confident_wrong: share(obvious.iter().filter(|o| !o.0 && o.1 >= t).count(), n),
        margin_correct,
        ambiguous_leak: share(amb.iter().filter(|&&m| m >= t).count(), amb.len()),
        flat_max: answers
            .iter()
            .filter(|a| a.kind == CaseKind::Flat)
            .map(|a| a.margin)
            .fold(0.0, f32::max),
        separation: margin_correct - mean(&amb),
        ambiguous_leak_acting: share(amb_acting, amb.len()),
        flat_max_acting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jev::probe::ProbeQuestion;

    const CRIT: &str = "self  hp:15(crit) armor:0 held:blaster ammo:50 low_ammo:no\n\
                        dmg   recent:85 since:now\n\
                        enemy visible:3 nearest:near moving:yes\n\
                        loc   exits:2 moving:yes\n\
                        items health:mid armor:none weapon:none powerup:none\n\
                        map   q2dm1 wants:health";

    #[test]
    fn jev_lab_baseline_is_identity_and_every_variant_is_deterministic() {
        let q = ProbeQuestion::Posture.spec();
        assert_eq!(
            Variant::Baseline.apply(CRIT, &q),
            (CRIT.to_string(), q.clone())
        );
        for v in Variant::ALL {
            let first = v.apply(CRIT, &q);
            for _ in 0..100 {
                assert_eq!(v.apply(CRIT, &q), first, "{v:?}");
            }
        }
    }

    #[test]
    fn jev_lab_baseline_matches_the_production_renderer() {
        use crate::jev::render::render_state;
        use brain::jev::state::{DecisionState, Field, LoadoutGap, MapFacts, MapId};
        let s = DecisionState::from_fields(vec![Field::Map(MapFacts {
            map: MapId::Q2dm1,
            gap: LoadoutGap::Armor,
        })]);
        let text = render_state(&s, usize::MAX).text;
        let q = ProbeQuestion::Objective.spec();
        assert_eq!(Variant::Baseline.apply(&text, &q).0, text);
    }

    #[test]
    fn jev_lab_variants_change_exactly_what_they_claim() {
        let q = ProbeQuestion::Posture.spec();
        let lines = |v: Variant| v.apply(CRIT, &q).0.lines().count();
        assert_eq!(lines(Variant::DropDmg), 5);
        assert!(!Variant::DropDmg.apply(CRIT, &q).0.contains("dmg "));
        assert!(!Variant::DropMap.apply(CRIT, &q).0.contains("wants:"));
        assert!(Variant::HpBandOnly
            .apply(CRIT, &q)
            .0
            .starts_with("self  hp:crit armor:0"));
        assert!(Variant::AddThreat
            .apply(CRIT, &q)
            .0
            .ends_with("\nthreat high"));
        assert!(Variant::AddSummary
            .apply(CRIT, &q)
            .0
            .ends_with("summary: badly hurt, weak weapon, enemies close"));
        // Question-side variants leave the state alone and keep every label.
        for v in [
            Variant::OptionsConsequence,
            Variant::InstructionCriterion,
            Variant::InstructionRole,
        ] {
            let (text, q2) = v.apply(CRIT, &q);
            assert_eq!(text, CRIT, "{v:?}");
            let labels = |q: &QuestionSpec| match &q.kind {
                QuestionKind::Choice(o) => o.iter().map(|o| o.label).collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            assert_eq!(labels(&q2), labels(&q), "{v:?} keeps the labels");
            assert_ne!(q2, q, "{v:?} changed something");
        }
    }

    #[test]
    fn jev_lab_threat_levels() {
        let f = |t: &str| threat(&Facts::parse(t));
        assert_eq!(f(CRIT), "high");
        assert_eq!(
            f("self  hp:100(hi) armor:100 held:railgun\nenemy visible:1 nearest:far moving:no"),
            "low"
        );
        assert_eq!(
            f("self  hp:100(hi) armor:100 held:railgun\nenemy visible:0 nearest:none"),
            "none"
        );
    }

    #[test]
    fn jev_lab_split_is_stable_balanced_and_guarded() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325, "FNV-1a offset basis");
        assert_eq!(
            fnv1a(b"a"),
            0xaf63_dc4c_8601_ec8c,
            "FNV-1a 64 reference vector"
        );
        let names: Vec<String> = (0..200).map(|i| format!("{i:02}_case.txt")).collect();
        let holdout = names.iter().filter(|n| half_of(n) == Half::Holdout).count();
        assert!((60..=140).contains(&holdout), "roughly half: {holdout}/200");
        assert_eq!(
            half_of("01_crit_blaster_3near.txt"),
            half_of("01_crit_blaster_3near.txt")
        );
        assert!(guard_half(Half::Holdout, false).is_err());
        assert!(guard_half(Half::Holdout, true).is_ok());
        assert!(guard_half(Half::Tuning, false).is_ok());
    }

    #[test]
    fn jev_lab_scoring_matches_hand_computation() {
        let a = |kind, top, margin| Scored { kind, top, margin };
        let ob = |l| CaseKind::Obvious(l);
        let answers = [
            a(ob("disengage"), "disengage", 0.8), // right, confident
            a(ob("disengage"), "disengage", 0.3), // right, not confident
            a(ob("hold"), "disengage", 0.6),      // wrong, confident
            a(ob("hold"), "disengage", 0.1),      // wrong, quiet
            a(CaseKind::Ambiguous, "hold", 0.55), // leaks
            a(CaseKind::Ambiguous, "hold", 0.05),
            a(CaseKind::Flat, "hold", 0.2),
        ];
        let m = score(&answers, 0.5, Some("hold"));
        assert_eq!(m.obvious, 4);
        assert_eq!(
            (m.accuracy, m.right_confident, m.confident_wrong),
            (0.5, 0.25, 0.25)
        );
        assert!((m.margin_correct - 0.55).abs() < 1e-6);
        assert_eq!(m.ambiguous_leak, 0.5);
        assert_eq!(m.flat_max, 0.2);
        assert!((m.separation - (0.55 - 0.30)).abs() < 1e-6);
        // The leaking ambiguous answer was a no-op `hold`; the flat one too.
        assert_eq!((m.ambiguous_leak_acting, m.flat_max_acting), (0.0, 0.0));
        let flat_disengage = [
            a(CaseKind::Flat, "disengage", 0.6),
            a(CaseKind::Ambiguous, "disengage", 0.7),
        ];
        let m = score(&flat_disengage, 0.5, Some("hold"));
        assert_eq!((m.ambiguous_leak_acting, m.flat_max_acting), (1.0, 0.6));
        // Without a no-op label every answer acts.
        assert_eq!(score(&answers, 0.5, None).ambiguous_leak_acting, 0.5);
    }

    #[test]
    fn jev_lab_corpus_loader_reads_labels_and_rejects_unknown_ones() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let q = ProbeQuestion::Posture.spec();
        let w = |p: &str, s: &str| {
            let path = tmp.path().join(p);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, s).expect("write");
        };
        w("obvious/disengage/01_a.txt", "self  hp:5(crit)\n");
        w("ambiguous/02_b.txt", "self  hp:50(mid)\n");
        w("flat/03_c.txt", "map   q2dm1 wants:none\n");
        let c = load_corpus(tmp.path(), &q).expect("loads");
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].kind, CaseKind::Obvious("disengage"));
        assert_eq!(c[0].text, "self  hp:5(crit)");
        w("obvious/charge/04_d.txt", "x\n");
        assert!(load_corpus(tmp.path(), &q).is_err());
    }
}
