# Human-like play — acceptance & measurement (Plan 47)

> Started 2026-07-10 (reordered ahead of the behavior plans 28/29/33). This file records the
> repeatable acceptance harness + baselines that gate the series goal.

## Why this exists — the variance lesson

Plan 30's live A/B nearly led us to revert a principled change on **noise**. A single 5-minute
competition's K/D is not signal: with `q3` as an **unchanged control group**, its K/D came out
**1.00, then 0.86, then 2.60** on three consecutive q2dm1 runs of identical code. Any main-side
change looked like a trend that was pure sampling variance (spawn draws, encounter luck, quad RNG).

**Rule:** never conclude a combat-behavior change helps/hurts from one run. Aggregate N runs and
compare the between-brain gap against the control group's own run-to-run spread. See
`context/pitfalls.md` → "Combat A/B on single competition runs is noise, not signal".

## The multi-run aggregator — `tools/acceptance`

```bash
# 1. Run the SAME competition N times (each writes a scoreboard log). 5+ recommended.
for i in $(seq 1 5); do
  timeout -s INT 305 qbots competition --count 3 --brains main,q3 --navmodes astar \
    --addr <host>:27910 > run_$i.log 2>&1
done
# 2. Aggregate. --control is the UNCHANGED brain; its spread is the noise floor.
cargo run -p tools --bin acceptance -- --control q3_astar run_*.log
```

Output: per-group `mean_kd [min..max] spread`, a **noise floor** (= control spread), and a
pairwise verdict — a `Δmean` **below** the noise floor is `noise — inconclusive`, above it is
`SIGNAL`. Parse/aggregate/verdict core is unit-tested (`crates/tools/src/bin/acceptance.rs`).

### Demonstration on real data (2026-07-10, q2dm1, 3 runs, q3 = identical-code control)

```
group                mean_kd  [min..max]   spread  runs  Σkills  Σdeaths
main_astar              0.49  [0.28..0.69]    0.41     3      17       37
q3_astar                1.49  [0.86..2.60]    1.74     3      32       25
noise floor (control spread) = 1.74 K/D
  main_astar vs q3_astar: Δmean=1.00  → noise — inconclusive
```

`main` *looks* far worse than `q3`, but the control's own spread (1.74) exceeds the 1.00 gap →
**inconclusive**. (These 3 runs also mixed pre/post-Plan-30 `main` code, so the `main` mean is not
a clean measurement — the point here is the tool + the control-variance floor. A real measurement
uses N runs of one code version.)

## Baseline — main vs q3 combat, q2dm1 (N=5, 2026-07-10)

First statistically-grounded competition baseline (commit at time: Plan-30 bounded main).
`competition --count 3 --brains main,q3 --navmodes astar` × 5 runs, 305 s each, aggregated:

```
group                mean_kd  [min..max]   spread  runs  Σkills  Σdeaths
main_astar              0.36  [0.00..0.67]    0.67     5      18       50
q3_astar                1.47  [1.18..1.80]    0.62     5      48       34
noise floor (control spread) = 0.62 K/D
  main_astar vs q3_astar: Δmean=1.12  → SIGNAL
```

**Read:** at N=5 the q3 control spread tightened to 0.62 (was a noise-dominated 1.74 at N=3), so the
1.12 main-vs-q3 gap is now real **SIGNAL** — `main` is reliably weaker than `q3`. This is the
**per-engagement combat-quality gap** (aim/dodge/fire cadence), not a navigation or resource gap
(Plan 30's resource work correctly didn't move it). **Regression contract:** a future change that
drops `main`'s N=5 mean below ~0.30, or widens the gap, must be investigated. Plans 28/29 (weapon
matchups, chase/disengage) target closing this gap — measure them against this baseline with N≥5.

> Not yet isolated: whether Plan 30 itself nudged `main` up/down vs a pre-Plan-30 N=5 (would need
> the pre-P30 binary re-run ×5). The gap is pre-existing (Plan 45: main 0.68 vs q3 1.3), so Plan
> 30's resource changes are groundwork, not the combat-quality fix.

## Still to build (Plan 47 remainder)

- **T1** behavior counters (`EVT switch/pickup/chase/ride/swim/drown`) + FleetStats aggregation.
- **T2** full acceptance driver: the traversal matrix (per brain: q2dm1 swim, q2dm3 railgun ≥3/4,
  q2dm3 quad, q2dm2 s2s 8/8) + the multi-run competition, one pass/fail table.
- **T3** recorded baseline table (date + commit) as the regression contract.
- **T4** 5-min showcase (persona roster) + "does it feel human" narrative.
