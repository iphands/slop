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

// ── Plan 74 T7: the pickup objective and per-life lines ──────────────────────────────────

/// One objective decision with its 30 s outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjEntry {
    /// `jev` or `jev_rule`.
    pub arm: String,
    /// The model's top choice.
    pub top: String,
    /// Did the model's answer pass the gate?
    pub decisive: bool,
    /// What the bot acted on (`None` = nothing).
    pub applied: Option<String>,
    /// The code rule's choice from the same candidates.
    pub rule: Option<String>,
    /// Reached the applied pad.
    pub arrived: bool,
    /// Held weapon tier rose.
    pub upgraded: bool,
    /// Why the objective ended inside the window (`None` = still held at close).
    pub released: Option<String>,
    /// Died inside the window.
    pub died: bool,
    /// Health delta.
    pub hp_delta: i32,
    /// Armor delta.
    pub armor_delta: i32,
}

/// Parse one journal line into an objective entry (`None` for anything else).
pub fn parse_objective(line: &str) -> Option<ObjEntry> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let ans = v["answers"]
        .as_array()?
        .iter()
        .find(|a| a["q"] == "objective")?
        .clone();
    let o = v.get("outcome")?;
    let ob = o.get("objective")?;
    let s = |x: &serde_json::Value| x.as_str().map(str::to_string);
    Some(ObjEntry {
        arm: v["arm"].as_str()?.to_string(),
        top: ans["top"].as_str().unwrap_or("?").to_string(),
        decisive: ans["gate"] == "apply",
        applied: s(&ob["applied"]),
        rule: s(&ob["rule"]),
        arrived: ob["arrived"].as_bool().unwrap_or(false),
        upgraded: ob["upgraded"].as_bool().unwrap_or(false),
        released: s(&ob["released"]),
        died: o["died"].as_bool().unwrap_or(false),
        hp_delta: o["hp_delta"].as_i64().unwrap_or(0) as i32,
        armor_delta: o["armor_delta"].as_i64().unwrap_or(0) as i32,
    })
}

/// One finished life (any arm).
#[derive(Clone, Debug, PartialEq)]
pub struct Life {
    /// The brain tag.
    pub arm: String,
    /// Seconds alive.
    pub secs: f64,
    /// Seconds to the first strong weapon, if any.
    pub secs_to_strong: Option<f64>,
    /// Frags in the life.
    pub frags: i64,
}

/// Parse a `{"kind":"life",…}` journal line.
pub fn parse_life(line: &str) -> Option<Life> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    (v["kind"] == "life").then_some(())?;
    Some(Life {
        arm: v["arm"].as_str()?.to_string(),
        secs: v["secs"].as_f64()?,
        secs_to_strong: v["secs_to_strong"].as_f64(),
        frags: v["frags"].as_i64().unwrap_or(0),
    })
}

fn pct_of(k: usize, n: usize) -> f64 {
    if n == 0 {
        0.0
    } else {
        100.0 * k as f64 / n as f64
    }
}

fn median(v: &mut [f64]) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(v[n / 2]),
        _ => Some((v[n / 2 - 1] + v[n / 2]) / 2.0),
    }
}

fn low_power(n: usize) -> &'static str {
    if n < MIN_DECISIONS {
        "  LOW POWER"
    } else {
        ""
    }
}

/// Per arm: lives, share that ever held a strong weapon, median `secs_to_strong` (over those),
/// mean life length, frags per life.
pub fn life_report(lives: &[Life]) -> String {
    let mut by: BTreeMap<&str, Vec<&Life>> = BTreeMap::new();
    for l in lives {
        by.entry(l.arm.as_str()).or_default().push(l);
    }
    let mut s = String::from("lives by arm (secs_to_strong = spawn → first non-weak weapon)\n");
    for (arm, ls) in by {
        let n = ls.len();
        let mut strong: Vec<f64> = ls.iter().filter_map(|l| l.secs_to_strong).collect();
        let got = strong.len();
        let med = median(&mut strong).map_or("—".to_string(), |m| format!("{m:.1}s"));
        let mean_secs = ls.iter().map(|l| l.secs).sum::<f64>() / n as f64;
        let fpl = ls.iter().map(|l| l.frags).sum::<i64>() as f64 / n as f64;
        let _ = writeln!(
            s,
            "  {arm:<12} lives={n:<4} got_strong={:>5.1}%  median_secs_to_strong={med:<7} \
             life={mean_secs:>5.1}s  frags/life={fpl:>4.2}{}",
            pct_of(got, n),
            low_power(n)
        );
    }
    s
}

/// Per arm × applied label: arrival, upgrade, death, hp/armor; and model/rule agreement on
/// decisive answers (Plan 74 P2).
pub fn objective_report(es: &[ObjEntry]) -> String {
    let mut s = String::from("objective decisions by arm × applied\n");
    let mut by: BTreeMap<String, Vec<&ObjEntry>> = BTreeMap::new();
    for e in es {
        let k = format!(
            "{} {}",
            e.arm,
            e.applied.as_deref().unwrap_or("(nothing applied)")
        );
        by.entry(k).or_default().push(e);
    }
    for (k, v) in &by {
        let n = v.len();
        let c = |f: fn(&ObjEntry) -> bool| v.iter().filter(|e| f(e)).count();
        let _ = writeln!(
            s,
            "  {k:<34} n={n:<4} arrived={:>5.1}%  upgraded={:>5.1}%  died={:>5.1}%  hp={:>+6.1}  \
             armor={:>+6.1}{}",
            pct_of(c(|e| e.arrived), n),
            pct_of(c(|e| e.upgraded), n),
            pct_of(c(|e| e.died), n),
            v.iter().map(|e| f64::from(e.hp_delta)).sum::<f64>() / n as f64,
            v.iter().map(|e| f64::from(e.armor_delta)).sum::<f64>() / n as f64,
            low_power(n)
        );
    }
    s.push_str("why objectives ended (applied seeks only)\n");
    let mut why: BTreeMap<String, usize> = BTreeMap::new();
    for e in es
        .iter()
        .filter(|e| e.applied.as_deref().is_some_and(|a| a != "carry_on"))
    {
        let r = e.released.as_deref().unwrap_or("held at close");
        *why.entry(format!("{} {r}", e.arm)).or_default() += 1;
    }
    for (k, n) in &why {
        let _ = writeln!(s, "  {k:<34} {n}");
    }
    s.push_str("model/rule agreement (decisive model answers)\n");
    let mut arms: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for e in es.iter().filter(|e| e.decisive && e.rule.is_some()) {
        let a = arms.entry(e.arm.as_str()).or_default();
        a.0 += 1;
        a.1 += usize::from(e.rule.as_deref() == Some(e.top.as_str()));
    }
    for (arm, (n, agree)) in arms {
        let _ = writeln!(
            s,
            "  {arm:<12} n={n:<4} agree={:>5.1}%{}",
            pct_of(agree, n),
            low_power(n)
        );
    }
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
    fn jev_journal_objective_and_life_sections() {
        let obj = |arm: &str, top: &str, applied: &str, rule: &str, arrived: bool| {
            serde_json::json!({
                "arm": arm, "trig": "released", "state": "",
                "answers": [{"q": "objective", "top": top, "margin": 0.5, "gate": "apply"}],
                "outcome": {"died": false, "hp_delta": 0, "armor_delta": 25, "frags_delta": 0,
                            "objective": {"applied": applied, "rule": rule, "arrived": arrived,
                                          "secs_to_arrive": 4.0, "upgraded": arrived}},
            })
            .to_string()
        };
        let lines = [
            obj("jev", "seek_weapon", "seek_weapon", "seek_weapon", true),
            obj("jev", "seek_armor", "seek_armor", "seek_weapon", false),
            obj("jev_rule", "carry_on", "seek_weapon", "seek_weapon", true),
            line("jev", "disengage", 0.6, false, false), // a posture line is not an objective
        ];
        let es: Vec<ObjEntry> = lines.iter().filter_map(|l| parse_objective(l)).collect();
        assert_eq!(es.len(), 3);
        let r = objective_report(&es);
        assert!(r.contains("jev seek_weapon"), "{r}");
        assert!(r.contains("jev_rule seek_weapon"), "{r}");
        assert!(r.contains("jev          n=2    agree= 50.0%"), "{r}");
        assert!(r.contains("jev_rule     n=1    agree=  0.0%"), "{r}");
        let life = |arm: &str, s2s: Option<f64>| {
            serde_json::json!({"kind": "life", "arm": arm, "secs": 30.0,
                               "secs_to_strong": s2s, "died": true, "frags": 1})
            .to_string()
        };
        let ls: Vec<Life> = [
            life("q3", None),
            life("q3", Some(20.0)),
            life("jev", Some(8.0)),
            line("jev", "hold", 0.2, false, false),
        ]
        .iter()
        .filter_map(|l| parse_life(l))
        .collect();
        assert_eq!(ls.len(), 3, "a decision line is not a life");
        let r = life_report(&ls);
        assert!(
            r.contains("q3           lives=2    got_strong= 50.0%  median_secs_to_strong=20.0s"),
            "{r}"
        );
        assert!(r.contains("LOW POWER"));
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
