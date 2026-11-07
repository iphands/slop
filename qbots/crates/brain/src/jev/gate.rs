//! # brain::jev::gate — when an answer is a decision (Plan 72 T3)
//!
//! Pure functions over one answer's probability vector. The **hub** calls them (the brain
//! never sees an ungated answer).
//!
//! **The gate statistic is the margin `p1 − p2`.** An `n_eff = exp(−Σ p ln p) ≤ 1.5` gate (the
//! plan's first draft) needs p1 ≥ 0.86 for every n ≤ 3 question — which is every question we
//! ask — and live answers to obviously-decidable states came back at 0.66 (plan *Gates*). So:
//! margin `≥ margin_binary` for n = 2, `≥ margin_multi` for n = 3, and for n ≥ 4 (never offered,
//! but the captured 4-way fixture exercises it) the margin **and** `n_eff ≤ max(1.5, frac·n)`.
//! Thresholds are provisional until W2's `jev-probe calibrate`, then frozen.

use super::decision::{Decision, DistStats, MalformedKind, NoSignalReason};

/// Probabilities must sum to 1 within this.
pub const SUM_TOLERANCE: f64 = 1e-3;

/// Gate thresholds. `Default` = the plan's provisional values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateThresholds {
    /// Minimum margin for a two-option question.
    pub margin_binary: f32,
    /// Minimum margin for three or more options.
    pub margin_multi: f32,
    /// For n ≥ 4 only: reject unless `n_eff ≤ max(1.5, neff_frac · n)`.
    pub neff_frac: f32,
}

impl Default for GateThresholds {
    fn default() -> Self {
        Self {
            margin_binary: 0.50,
            margin_multi: 0.30,
            neff_frac: 0.5,
        }
    }
}

/// `p1 − p2` (0 for fewer than two options).
pub fn margin(p: &[f32]) -> f32 {
    let (mut a, mut b) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for &x in p {
        if x > a {
            b = a;
            a = x;
        } else if x > b {
            b = x;
        }
    }
    if p.len() < 2 {
        0.0
    } else {
        a - b
    }
}

/// `exp(−Σ p ln p)`, with `0 ln 0 = 0`.
pub fn n_eff(p: &[f32]) -> f32 {
    let h: f64 = p
        .iter()
        .map(|&x| f64::from(x))
        .filter(|&x| x > 0.0)
        .map(|x| -x * x.ln())
        .sum();
    h.exp() as f32
}

/// All statistics for one vector; `None` if it is empty.
pub fn stats(p: &[f32]) -> Option<DistStats> {
    let (top, &p1) = p
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.total_cmp(b))?;
    Some(DistStats {
        n: p.len(),
        top,
        p1,
        margin: margin(p),
        n_eff: n_eff(p),
        excess: p1 - 1.0 / p.len() as f32,
    })
}

/// Are these probabilities well-formed: finite, non-negative, summing to 1 (±[`SUM_TOLERANCE`])?
pub fn well_formed(p: &[f32]) -> bool {
    let finite_nonneg = p.iter().all(|x| x.is_finite() && *x >= 0.0);
    let sum: f64 = p.iter().map(|&x| f64::from(x)).sum();
    finite_nonneg && (sum - 1.0).abs() <= SUM_TOLERANCE
}

/// Classify one answer's probabilities: `Apply(index of the chosen option)`, `NoSignal`, or
/// `Malformed(BadProbabilities)`.
pub fn gate(p: &[f32], th: &GateThresholds) -> Decision<usize> {
    if !well_formed(p) {
        return Decision::Malformed(MalformedKind::BadProbabilities);
    }
    let Some(st) = stats(p) else {
        return Decision::Malformed(MalformedKind::BadProbabilities);
    };
    if st.n < 2 {
        return Decision::NoSignal {
            reason: NoSignalReason::TooFewOptions { n: st.n },
            stats: st,
        };
    }
    let min_margin = if st.n == 2 {
        th.margin_binary
    } else {
        th.margin_multi
    };
    let neff_ok = st.n < 4 || st.n_eff <= (th.neff_frac * st.n as f32).max(1.5);
    if st.margin >= min_margin && neff_ok {
        Decision::Apply(st.top)
    } else {
        Decision::NoSignal {
            reason: NoSignalReason::Flat {
                margin: st.margin,
                n_eff: st.n_eff,
            },
            stats: st,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn th() -> GateThresholds {
        GateThresholds::default()
    }

    /// Assert a `Flat` verdict with the given statistics (±0.005).
    fn assert_flat(d: Decision<usize>, margin_want: f32, neff_want: f32) {
        match d {
            Decision::NoSignal {
                reason: NoSignalReason::Flat { margin, n_eff },
                ..
            } => {
                assert!((margin - margin_want).abs() < 0.005, "margin {margin}");
                assert!((n_eff - neff_want).abs() < 0.005, "n_eff {n_eff}");
            }
            other => panic!("expected Flat, got {other:?}"),
        }
    }

    #[test]
    fn jev_gate_captured_flat_four_way_fixture_is_rejected() {
        // objective {hunt, hold_area, roam, seek_item} — the plan's most valuable fixture.
        assert_flat(gate(&[0.3164, 0.1040, 0.4911, 0.0885], &th()), 0.175, 3.20);
    }

    #[test]
    fn jev_gate_live_answers_2026_11_07() {
        // "cannot win this fight": disengage 0.880 — decidable.
        assert_eq!(gate(&[0.880, 0.120], &th()), Decision::Apply(0));
        // hp 18, no armor, 2 enemies, binary: margin 0.326 < 0.50 — provisional gate rejects.
        assert_flat(gate(&[0.663, 0.337], &th()), 0.326, 1.895);
        // same state, engage/trade/disengage: margin 0.064.
        assert_flat(gate(&[0.182, 0.377, 0.441], &th()), 0.064, 2.826);
    }

    #[test]
    fn jev_gate_regression_first_draft_neff_gate_rejected_a_clear_binary() {
        // n_eff {0.80, 0.20} = 1.65 > 1.5: the first draft rejected this. Margin 0.60 applies.
        assert!((n_eff(&[0.80, 0.20]) - 1.649).abs() < 0.005);
        assert_eq!(gate(&[0.80, 0.20], &th()), Decision::Apply(0));
        assert_eq!(gate(&[0.20, 0.80], &th()), Decision::Apply(1));
    }

    #[test]
    fn jev_gate_three_way_uses_the_multi_margin() {
        assert_eq!(gate(&[0.20, 0.15, 0.65], &th()), Decision::Apply(2)); // margin 0.45
        assert_flat(gate(&[0.45, 0.20, 0.35], &th()), 0.10, 2.854);
    }

    #[test]
    fn jev_gate_four_way_needs_margin_and_neff() {
        // Confident: margin 0.85, n_eff ≈ 1.53 ≤ 2.
        assert_eq!(gate(&[0.90, 0.05, 0.03, 0.02], &th()), Decision::Apply(0));
        // Margin 0.33 passes but the mass is spread: n_eff ≈ 3.46 > 2 ⇒ Flat.
        assert_flat(gate(&[0.50, 0.17, 0.17, 0.16], &th()), 0.33, 3.463);
    }

    #[test]
    fn jev_gate_tie_is_flat() {
        assert_flat(gate(&[0.5, 0.5], &th()), 0.0, 2.0);
    }

    #[test]
    fn jev_gate_too_few_options() {
        match gate(&[1.0], &th()) {
            Decision::NoSignal {
                reason: NoSignalReason::TooFewOptions { n: 1 },
                ..
            } => {}
            other => panic!("expected TooFewOptions, got {other:?}"),
        }
    }

    #[test]
    fn jev_gate_bad_probabilities_are_malformed_never_gated() {
        let bad = Decision::Malformed(MalformedKind::BadProbabilities);
        assert_eq!(gate(&[0.6, 0.3], &th()), bad, "sums to 0.9");
        assert_eq!(gate(&[f32::NAN, 0.5], &th()), bad);
        assert_eq!(gate(&[1.2, -0.2], &th()), bad);
        assert_eq!(gate(&[], &th()), bad);
        // Within the 1e-3 tolerance is fine.
        assert_eq!(gate(&[0.9004, 0.1], &th()), Decision::Apply(0));
    }

    #[test]
    fn jev_gate_stats_carry_the_secondaries() {
        let s = stats(&[0.051, 0.949]).expect("non-empty");
        assert_eq!((s.n, s.top), (2, 1));
        assert!((s.excess - 0.449).abs() < 1e-4);
    }
}
