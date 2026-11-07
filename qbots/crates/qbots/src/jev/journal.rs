//! # qbots::jev::journal — read the arena's decision journal (Plan 73 T7)
//!
//! One journal line = one decision: the arm, the trigger, the rendered state, the answer (top,
//! margin, gate) and the **outcome** window (died, hp/armor/frags deltas, damage taken, q3's own
//! retreat wish, whether the bias was in force). This module turns lines into the tables we learn
//! from:
//!
//! - per arm × answer: how often the bot died in the window, its hp and frag deltas;
//! - **agree / disagree** with q3: the model's answer against what q3 alone wanted that tick;
//! - margin bands: are confident answers followed by better outcomes?
//! - state words (summary hurt / weapon / enemies, threat): which situations go badly;
//! - the worst outcomes, written out as **candidate scenarios** for a human to label.
//!
//! Comparisons with fewer than [`MIN_DECISIONS`] decisions are flagged **LOW POWER** — they are
//! hypotheses for the lab, not results (plan guard rail 5).

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Below this many decisions a comparison is flagged LOW POWER.
pub const MIN_DECISIONS: usize = 100;

/// One posture decision with its outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// `jev` or `jev_static` (shadow).
    pub arm: String,
    /// The trigger slug.
    pub trig: String,
    /// The rendered state text.
    pub state: String,
    /// The model's top choice.
    pub top: String,
    /// `p1 − p2`.
    pub margin: f32,
    /// Did the answer pass the gate?
    pub applied: bool,
    /// Died inside the window.
    pub died: bool,
    /// Health delta over the window.
    pub hp_delta: i32,
    /// Frags delta over the window.
    pub frags_delta: i32,
    /// q3's own retreat wish at the decision.
    pub q3_retreat: Option<bool>,
}

/// Parse one journal line into its posture entry (`None` for other lines / no outcome).
pub fn parse_line(line: &str) -> Option<Entry> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let ans = v["answers"]
        .as_array()?
        .iter()
        .find(|a| a["q"] == "posture")?
        .clone();
    let o = v.get("outcome")?;
    Some(Entry {
        arm: v["arm"].as_str()?.to_string(),
        trig: v["trig"].as_str().unwrap_or("?").to_string(),
        state: v["state"].as_str().unwrap_or_default().to_string(),
        top: ans["top"].as_str().unwrap_or("?").to_string(),
        margin: ans["margin"].as_f64().unwrap_or(0.0) as f32,
        applied: ans["gate"] == "apply",
        died: o["died"].as_bool().unwrap_or(false),
        hp_delta: o["hp_delta"].as_i64().unwrap_or(0) as i32,
        frags_delta: o["frags_delta"].as_i64().unwrap_or(0) as i32,
        q3_retreat: o["q3_retreat"].as_bool(),
    })
}

/// Outcome aggregate over a group of decisions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Agg {
    /// Decisions.
    pub n: usize,
    /// Died inside the window.
    pub died: usize,
    /// Σ hp delta.
    pub hp: i64,
    /// Σ frag delta.
    pub frags: i64,
}

impl Agg {
    fn add(&mut self, e: &Entry) {
        self.n += 1;
        self.died += usize::from(e.died);
        self.hp += i64::from(e.hp_delta);
        self.frags += i64::from(e.frags_delta);
    }

    /// Share of decisions followed by a death in the window.
    pub fn died_rate(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.died as f64 / self.n as f64
        }
    }

    fn row(&self, label: &str) -> String {
        let mean = |x: i64| {
            if self.n == 0 {
                0.0
            } else {
                x as f64 / self.n as f64
            }
        };
        format!(
            "  {label:<44} n={:<4} died={:>5.1}%  hp={:>+6.1}  frags={:>+5.2}{}",
            self.n,
            100.0 * self.died_rate(),
            mean(self.hp),
            mean(self.frags),
            if self.n < MIN_DECISIONS {
                "  LOW POWER"
            } else {
                ""
            }
        )
    }
}

fn group<'a>(
    entries: &'a [Entry],
    key: impl Fn(&'a Entry) -> Option<String>,
) -> BTreeMap<String, Agg> {
    let mut m: BTreeMap<String, Agg> = BTreeMap::new();
    for e in entries {
        if let Some(k) = key(e) {
            m.entry(k).or_default().add(e);
        }
    }
    m
}

/// The words of the state's `summary:` and `threat` lines, e.g. `["hurt", "weak weapon", …]`.
pub fn state_words(state: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in state.lines() {
        if let Some(rest) = line.strip_prefix("summary: ") {
            out.extend(rest.split(", ").map(str::to_string));
        } else if let Some(t) = line.strip_prefix("threat ") {
            out.push(format!("threat {t}"));
        }
    }
    out
}

fn band(m: f32) -> &'static str {
    if m < 0.3 {
        "margin <0.30"
    } else if m < 0.5 {
        "margin 0.30–0.50"
    } else {
        "margin ≥0.50"
    }
}

fn retreat(q: Option<bool>) -> &'static str {
    match q {
        Some(true) => "q3 retreat",
        Some(false) => "q3 fight",
        None => "q3 ?",
    }
}

/// The full text report.
pub fn report(entries: &[Entry]) -> String {
    let mut s = String::new();
    let section = |s: &mut String, title: &str, m: BTreeMap<String, Agg>| {
        let _ = writeln!(s, "{title}");
        for (k, a) in &m {
            let _ = writeln!(s, "{}", a.row(k));
        }
    };
    let _ = writeln!(
        s,
        "jev journal: {} posture decisions with outcomes",
        entries.len()
    );
    section(
        &mut s,
        "by arm × answer",
        group(entries, |e| {
            Some(format!(
                "{} {} ({})",
                e.arm,
                e.top,
                if e.applied { "apply" } else { "no_signal" }
            ))
        }),
    );
    section(
        &mut s,
        "by arm × answer × q3's own wish (agree / disagree)",
        group(entries, |e| {
            Some(format!("{} {} | {}", e.arm, e.top, retreat(e.q3_retreat)))
        }),
    );
    section(
        &mut s,
        "by arm × answer × margin band",
        group(entries, |e| {
            Some(format!("{} {} | {}", e.arm, e.top, band(e.margin)))
        }),
    );
    section(
        &mut s,
        "by arm × trigger",
        group(entries, |e| Some(format!("{} {}", e.arm, e.trig))),
    );
    let mut words: BTreeMap<String, Agg> = BTreeMap::new();
    for e in entries {
        for w in state_words(&e.state) {
            words
                .entry(format!("{} {} | {w}", e.arm, e.top))
                .or_default()
                .add(e);
        }
    }
    section(&mut s, "by arm × answer × state word", words);
    s
}

/// The worst outcomes — confident answers followed by a death — as scenario candidates for a
/// human to label: `(file stem, state text)`.
pub fn candidates(entries: &[Entry], limit: usize) -> Vec<(String, String)> {
    let mut bad: Vec<&Entry> = entries.iter().filter(|e| e.died && e.applied).collect();
    bad.sort_by(|a, b| b.margin.total_cmp(&a.margin));
    bad.iter()
        .take(limit)
        .enumerate()
        .map(|(i, e)| {
            (
                format!(
                    "{:02}_{}_{}_died_m{:03}",
                    i + 1,
                    e.arm,
                    e.top,
                    (e.margin * 100.0) as u32
                ),
                e.state.clone(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(arm: &str, top: &str, margin: f32, died: bool, q3: bool) -> String {
        serde_json::json!({
            "arm": arm, "trig": "contact",
            "state": "self  hp:40(lo)\nthreat high\nsummary: hurt, weak weapon, enemies close",
            "answers": [{"q": "posture", "top": top, "margin": margin, "gate": "apply"}],
            "outcome": {"died": died, "hp_delta": if died { -40 } else { 0 }, "frags_delta": 0,
                        "q3_retreat": q3},
        })
        .to_string()
    }

    #[test]
    fn jev_journal_report_aggregates_known_answers() {
        let lines = [
            line("jev", "disengage", 0.6, false, false),
            line("jev", "disengage", 0.4, true, false),
            line("jev_static", "hold", 0.2, true, true),
            "not json".to_string(),
            serde_json::json!({"arm": "jev", "answers": []}).to_string(),
        ];
        let entries: Vec<Entry> = lines.iter().filter_map(|l| parse_line(l)).collect();
        assert_eq!(
            entries.len(),
            3,
            "malformed and outcome-less lines are skipped"
        );
        let by_arm = group(&entries, |e| Some(e.arm.clone()));
        assert_eq!(by_arm["jev"].n, 2);
        assert_eq!(by_arm["jev"].died_rate(), 0.5);
        let r = report(&entries);
        assert!(r.contains("jev disengage (apply)"), "{r}");
        assert!(r.contains("jev disengage | q3 fight"), "{r}");
        assert!(r.contains("jev disengage | margin ≥0.50"), "{r}");
        assert!(r.contains("| weak weapon"), "{r}");
        assert!(r.contains("LOW POWER"), "n < 100 is flagged");
        assert_eq!(
            state_words("summary: hurt, weak weapon, enemies close\nthreat high"),
            ["hurt", "weak weapon", "enemies close", "threat high"]
        );
    }

    #[test]
    fn jev_journal_candidates_are_confident_deaths_first() {
        let entries: Vec<Entry> = [
            line("jev", "disengage", 0.4, true, false),
            line("jev", "hold", 0.9, true, true),
            line("jev", "disengage", 0.7, false, false),
        ]
        .iter()
        .filter_map(|l| parse_line(l))
        .collect();
        let c = candidates(&entries, 10);
        assert_eq!(c.len(), 2, "only deaths");
        assert!(c[0].0.starts_with("01_jev_hold_died_m090"), "{}", c[0].0);
    }
}
