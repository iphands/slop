//! # acceptance — multi-run competition aggregator (Plan 47, reordered 2026-07-10)
//!
//! Plan 30's live A/B taught us the hard way that a **single** 5-minute competition is statistical
//! noise, not signal: a `q3` CONTROL group's K/D swung 1.00 → 0.86 → 2.60 across three runs of
//! *identical code* (see `context/pitfalls.md`). So no combat-behavior change (Plans 28/29/33) can
//! be judged from one run. This tool is the fix: feed it the logs of **N** competition runs and it
//! aggregates each group's K/D into `mean [min..max]`, then prints a **signal-vs-noise verdict**
//! using the control group's own run-to-run spread as the noise floor — a between-brain difference
//! smaller than the control's spread is not trustworthy.
//!
//! ## Usage
//! ```text
//! # 1. Run the same competition N times (each writes a scoreboard log), e.g.:
//! for i in 1 2 3 4 5; do
//!   timeout -s INT 305 qbots competition --count 3 --brains main,q3 --navmodes astar \
//!     --addr host:27910 > run_$i.log 2>&1
//! done
//! # 2. Aggregate — the control is the UNCHANGED brain (its spread = the noise floor):
//! acceptance --control q3_astar run_1.log run_2.log run_3.log run_4.log run_5.log
//! ```
//!
//! The parsing/aggregation core is pure + unit-tested; `main` is just file IO + printing.

use std::collections::BTreeMap;
use std::process::ExitCode;

/// One group's kills/deaths as parsed from a single run's FINAL scoreboard line.
#[derive(Debug, Clone, PartialEq)]
struct GroupResult {
    name: String,
    kills: u32,
    deaths: u32,
}

/// K/D with the Q2 convention that 0 deaths counts as 1 (avoid div-by-zero; matches the
/// competition scoreboard's own `kd`).
fn kd(kills: u32, deaths: u32) -> f32 {
    kills as f32 / deaths.max(1) as f32
}

/// Parse a `key=<u32>` token out of a whitespace-tokenized line (`kills=13` → `13`).
fn field_u32(line: &str, key: &str) -> Option<u32> {
    line.split_whitespace()
        .find_map(|t| t.strip_prefix(key))
        .and_then(|v| v.parse().ok())
}

/// Parse one scoreboard group row, e.g. `... #2  main_astar bots=3  kills=5  deaths=18  kd=0.28`.
/// Returns `None` for any line without a `#<rank>` marker followed by a name and `kills=`/`deaths=`.
fn parse_group_line(line: &str) -> Option<GroupResult> {
    let name = line
        .split_whitespace()
        .skip_while(|t| !t.starts_with('#'))
        .nth(1)? // the token right after "#<rank>"
        .to_string();
    let kills = field_u32(line, "kills=")?;
    let deaths = field_u32(line, "deaths=")?;
    Some(GroupResult {
        name,
        kills,
        deaths,
    })
}

/// Extract the **final** scoreboard from one run's log: the group rows after the last `[FINAL]`
/// marker (the live scoreboards printed every 30 s are ignored). Falls back to the last `scoreboard`
/// block if the run was cut before printing `[FINAL]`.
fn parse_final_scoreboard(log: &str) -> Vec<GroupResult> {
    let lines: Vec<&str> = log.lines().collect();
    let anchor = lines
        .iter()
        .rposition(|l| l.contains("[FINAL]"))
        .or_else(|| lines.iter().rposition(|l| l.contains("scoreboard")));
    let start = anchor.map(|i| i + 1).unwrap_or(0);
    lines[start..]
        .iter()
        .map_while(|l| {
            // Stop at the first non-group line after the header (e.g. "competition exited").
            if l.contains("scoreboard") {
                return None;
            }
            Some(parse_group_line(l))
        })
        .flatten()
        .collect()
}

/// Aggregated K/D for one group across all runs.
#[derive(Debug, Clone, PartialEq)]
struct GroupAgg {
    name: String,
    kds: Vec<f32>,
    total_kills: u32,
    total_deaths: u32,
}

impl GroupAgg {
    fn mean_kd(&self) -> f32 {
        if self.kds.is_empty() {
            return 0.0;
        }
        self.kds.iter().sum::<f32>() / self.kds.len() as f32
    }
    fn min_kd(&self) -> f32 {
        self.kds.iter().copied().fold(f32::INFINITY, f32::min)
    }
    fn max_kd(&self) -> f32 {
        self.kds.iter().copied().fold(f32::NEG_INFINITY, f32::max)
    }
    /// Run-to-run spread (max − min) — the noise band for this group.
    fn spread(&self) -> f32 {
        if self.kds.is_empty() {
            0.0
        } else {
            self.max_kd() - self.min_kd()
        }
    }
}

/// Aggregate N runs (each a group-result list) into per-group K/D stats, keyed by group name.
fn aggregate(runs: &[Vec<GroupResult>]) -> Vec<GroupAgg> {
    let mut by_name: BTreeMap<String, GroupAgg> = BTreeMap::new();
    for run in runs {
        for g in run {
            let e = by_name.entry(g.name.clone()).or_insert_with(|| GroupAgg {
                name: g.name.clone(),
                kds: Vec::new(),
                total_kills: 0,
                total_deaths: 0,
            });
            e.kds.push(kd(g.kills, g.deaths));
            e.total_kills += g.kills;
            e.total_deaths += g.deaths;
        }
    }
    by_name.into_values().collect()
}

/// Format the aggregate table + a signal-vs-noise verdict. `control` names the group whose spread
/// is the noise floor (the UNCHANGED brain); if absent, the widest group spread is used.
fn report(aggs: &[GroupAgg], control: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("group                mean_kd  [min..max]   spread  runs  Σkills  Σdeaths\n");
    for a in aggs {
        out.push_str(&format!(
            "{:<20} {:>7.2}  [{:.2}..{:.2}]  {:>6.2}  {:>4}  {:>6}  {:>7}\n",
            a.name,
            a.mean_kd(),
            a.min_kd(),
            a.max_kd(),
            a.spread(),
            a.kds.len(),
            a.total_kills,
            a.total_deaths,
        ));
    }
    // Noise floor = the control group's spread (or the max spread across groups).
    let noise = control
        .and_then(|c| aggs.iter().find(|a| a.name == c))
        .map(|a| a.spread())
        .unwrap_or_else(|| aggs.iter().map(|a| a.spread()).fold(0.0, f32::max));
    out.push_str(&format!(
        "\nnoise floor (control spread) = {noise:.2} K/D\n"
    ));
    // Pairwise verdicts: a mean-K/D gap smaller than the noise floor is NOT trustworthy.
    for (i, a) in aggs.iter().enumerate() {
        for b in &aggs[i + 1..] {
            let gap = (a.mean_kd() - b.mean_kd()).abs();
            let verdict = if gap > noise {
                "SIGNAL"
            } else {
                "noise — inconclusive"
            };
            out.push_str(&format!(
                "  {} vs {}: Δmean={:.2}  → {verdict}\n",
                a.name, b.name, gap
            ));
        }
    }
    out
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut control: Option<String> = None;
    if let Some(pos) = args.iter().position(|a| a == "--control") {
        control = args.get(pos + 1).cloned();
        args.drain(pos..=pos + 1);
    }
    if args.is_empty() {
        eprintln!(
            "usage: acceptance [--control <group>] <run1.log> <run2.log> ...\n\
             aggregates N competition scoreboards into mean±spread K/D + a signal-vs-noise verdict."
        );
        return ExitCode::from(2);
    }
    let mut runs: Vec<Vec<GroupResult>> = Vec::new();
    for path in &args {
        match std::fs::read_to_string(path) {
            Ok(log) => {
                let board = parse_final_scoreboard(&log);
                if board.is_empty() {
                    eprintln!("[warn] no scoreboard parsed from {path}");
                } else {
                    runs.push(board);
                }
            }
            Err(e) => {
                eprintln!("[error] reading {path}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    if runs.is_empty() {
        eprintln!("[error] no usable runs");
        return ExitCode::FAILURE;
    }
    let aggs = aggregate(&runs);
    print!("{}", report(&aggs, control.as_deref()));
    println!(
        "\n{} runs aggregated. Reminder: treat a difference below the noise floor as inconclusive.",
        runs.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
0182.324 I ── competition scoreboard [live] (group: kills/deaths, K/D) ──
0182.324 I   #1  q3_astar  bots=3  kills=11   deaths=27   kd=0.41
0182.324 I   #2  main_astar bots=3  kills=4    deaths=21   kd=0.19
0305.142 I ── competition scoreboard [FINAL] (group: kills/deaths, K/D) ──
0305.142 I   #1  q3_astar  bots=3  kills=18   deaths=42   kd=0.43
0305.142 I   #2  main_astar bots=3  kills=5    deaths=40   kd=0.12
0305.142 I competition exited
";

    #[test]
    fn parses_only_the_final_board() {
        let board = parse_final_scoreboard(LOG);
        assert_eq!(
            board.len(),
            2,
            "two groups in the FINAL board (not the live one)"
        );
        assert_eq!(
            board[0],
            GroupResult {
                name: "q3_astar".into(),
                kills: 18,
                deaths: 42
            }
        );
        assert_eq!(board[1].name, "main_astar");
        assert_eq!(board[1].kills, 5);
        assert_eq!(board[1].deaths, 40);
    }

    #[test]
    fn group_line_rejects_non_rows() {
        assert!(parse_group_line("0305.142 I competition exited").is_none());
        assert!(parse_group_line("random text").is_none());
    }

    #[test]
    fn aggregates_mean_and_spread_across_runs() {
        // q3 (control) K/D over 3 runs: 1.00, 0.86, 2.60 — the real Plan 30 variance.
        let runs = vec![
            vec![
                GroupResult {
                    name: "q3".into(),
                    kills: 13,
                    deaths: 13,
                }, // 1.00
                GroupResult {
                    name: "main".into(),
                    kills: 9,
                    deaths: 13,
                }, // 0.69
            ],
            vec![
                GroupResult {
                    name: "q3".into(),
                    kills: 6,
                    deaths: 7,
                }, // 0.857
                GroupResult {
                    name: "main".into(),
                    kills: 3,
                    deaths: 6,
                }, // 0.50
            ],
            vec![
                GroupResult {
                    name: "q3".into(),
                    kills: 13,
                    deaths: 5,
                }, // 2.60
                GroupResult {
                    name: "main".into(),
                    kills: 5,
                    deaths: 18,
                }, // 0.278
            ],
        ];
        let aggs = aggregate(&runs);
        let q3 = aggs.iter().find(|a| a.name == "q3").unwrap();
        assert_eq!(q3.kds.len(), 3);
        assert!(
            q3.spread() > 1.7,
            "q3 control spread is huge: {}",
            q3.spread()
        );
    }

    #[test]
    fn verdict_calls_a_small_gap_noise() {
        // The exact Plan 30 numbers: main's mean is well below q3's, but q3's OWN spread (1.74) is
        // larger than the gap → the comparison must be declared inconclusive, not a regression.
        let runs = vec![
            vec![
                GroupResult {
                    name: "q3".into(),
                    kills: 13,
                    deaths: 13,
                },
                GroupResult {
                    name: "main".into(),
                    kills: 9,
                    deaths: 13,
                },
            ],
            vec![
                GroupResult {
                    name: "q3".into(),
                    kills: 13,
                    deaths: 5,
                },
                GroupResult {
                    name: "main".into(),
                    kills: 5,
                    deaths: 18,
                },
            ],
        ];
        let aggs = aggregate(&runs);
        let out = report(&aggs, Some("q3"));
        assert!(
            out.contains("noise — inconclusive"),
            "q3's spread dwarfs the main-vs-q3 gap → inconclusive; got:\n{out}"
        );
    }
}
