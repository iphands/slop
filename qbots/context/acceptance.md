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

## CURRENT combat baseline — main vs q3, q2dm1 (N=5, 2026-07-10, post-thrash-fix)

Re-run after the weapon-switch-thrash fix (the counters' first catch — the pre-fix N=5 below
was fire-suppressed):

```
group                mean_kd  [min..max]   spread  runs  Σkills  Σdeaths
main_astar              1.00  [0.67..1.27]    0.61     5      55       54
q3_astar                0.93  [0.50..1.50]    1.00     5      44       52
noise floor (control spread) = 1.00 K/D
  main_astar vs q3_astar: Δmean=0.07  → noise — inconclusive (= statistical PARITY)
```

**main has closed the historical combat gap** (Plan 45: 0.68 vs ~1.3; pre-fix today: 0.36 vs
1.47 SIGNAL deficit) — the cumulative behavior plans (28/29/30/27/33) + the thrash fix bring it
to parity, out-killing q3 in absolute terms (55 vs 44). **This table supersedes the one below as
the regression contract**: a change that reopens a SIGNAL-level main deficit at N≥5 must be
investigated.

## Superseded: pre-fix baseline — main vs q3 combat, q2dm1 (N=5, 2026-07-10)

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

## The traversal-matrix driver — `acceptance matrix` (2026-07-10)

One command runs the traversal gates per brain and prints a pass/fail table:

```bash
# All rows, all maps (prompts you to switch the server between map batches):
cargo run -p tools --bin acceptance -- matrix --addr <host>:27910 --brains runtester,main,q3
# One map batch, no prompts (wrong map fails fast on the scenario preflight):
cargo run -p tools --bin acceptance -- matrix --addr <host>:27910 --maps q2dm1 --yes
```

Rows (thresholds = proven floors, cited in-source): `swim-railgun` q2dm1 ≥2/3 ·
`ride-railgun` q2dm3 ≥3/4 · `quad-train-lava` q2dm3 ≥1/4 (target 3/4 pending Plan 35) ·
`spawn-to-spawn` q2dm2 ≥7/8 (unbaselined — tighten after first green run). The driver
regenerates the needed nav-cache variant (lift-penalty keyed) before each map batch and exits
non-zero on any FAIL — the regression gate is scriptable.

### FULL MATRIX BASELINE — 2026-07-10 (T3, the regression contract)

Live on `noir.lan:27910`, maps switched via RCON. Retry policy: one retry for a cell that
fails inside its documented variance band (used once, marked ®).

| row | map | brain | result | gate | verdict |
|---|---|---|---|---|---|
| swim-railgun | q2dm1 | runtester | 3/3 | ≥2/3 | **PASS** |
| swim-railgun | q2dm1 | main | 3/3 | ≥2/3 | **PASS** |
| ride-railgun | q2dm3 | runtester | 2/4 → 4/4 ® | ≥3/4 | **PASS** |
| ride-railgun | q2dm3 | main | 4/4 | ≥3/4 | **PASS** |
| ride-railgun | q2dm3 | q3 | 4/4 | ≥3/4 | **PASS** |
| quad-train-lava | q2dm3 | runtester | 2/4 | ≥1/4 | **PASS** |
| quad-train-lava | q2dm3 | main | 0/4, 0/4 ® | ≥1/4 | **FAIL** † |
| quad-train-lava | q2dm3 | q3 | 1/4 | ≥1/4 | **PASS** |
| spawn-to-spawn | q2dm2 | runtester | 3/8 (180s) | ≥3/8 | **PASS** |
| spawn-to-spawn | q2dm2 | main | 6/8 (180s) | ≥3/8 | **PASS** |
| spawn-to-spawn | q2dm2 | q3 | 4/8 (180s) | ≥3/8 | **PASS** |

(q2dm1 swim × q3 = 3/3, proven in the Plan 46 closeout matrix — not re-run here.)

**Regression contract:** a future change that drops a green cell below its gate must fix or
explicitly re-baseline with a written rationale.

**Named findings from the baseline run (follow-up work, tracked here):**
1. **† main cannot complete the quad `*10` ride from far spawns (0/8 across two runs)** while
   runtester (2/4) and q3 (1/4) can. main's steering lacks runtester's 7-ray backoff/escape on
   the fragmented q2dm3 upper-level approach (consistent with the Plan 35 wedging diagnosis) —
   a main-steering follow-up, not a TraversalExecutor bug (main's ride-railgun is 4/4).
2. **q2dm2 farthest-spawn routes are slow/unreliable for every brain** (3–6/8 even at 180s;
   a 90s cap saw 1–4/8). Connectivity is "full" but route quality isn't — the recurring
   "connectivity ≠ navigable" pitfall. A Plan-35-family nav-quality item.
3. runtester is consistently the *worst* q2dm2 s2s performer (1/8 @90s, 3/8 @180s) despite
   being the movement specialist — its corner-cut-safe pursue + rich backoff may cost wall
   time on long treks. Worth a recorder-log comparison against main's 6/8.

## Behavior counters (T1) — greppable `EVT` events

`grep -c "EVT <name>"` over any competition/fleet log:
`EVT switch weapon=<w> dist=<d>` · `EVT chase start|convert|abort reason=…` ·
`EVT traverse done kind=swim|ride|ladder` · `EVT drown` (gate: zero).
Pickup counters + FleetStats aggregation deferred (no direct wire signal for pickups).

## Showcase (T4) — 5-min `main` vs `q3`, q2dm3, 2026-07-10

**The counters earned their keep on their first run**: showcase #1 exposed **4179 weapon
switches in 305 s** (~14/s Blaster↔Railgun thrash — the Plan 30 dry-gate read the WIRE-held
weapon's `STAT_AMMO` but gated the *optimistic* held model; each request also reset the fire
lockout, so a thrashing bot barely fired — present in every run since P30-T4, **including the
N=5 main baseline, which should be re-run**). Fixed (wire re-sync + 1 s request cooldown) and
re-run:

| counter | showcase #1 (buggy) | showcase #2 (fixed) |
|---|---|---|
| EVT switch | 4179 | **493** (sane ranged Railgun requests) |
| EVT chase start / convert / abort | 305 / 149 / 11 | 239 / **89** / 10 (all aborts = third-party) |
| EVT traverse done | 24 (18 ladder, 6 ride) | **44** (35 ladder, 9 ride) |
| EVT drown | **0** | **0** |
| scoreboard | q3 0.28, main 0.22 | q3 14k/57d **0.25**, main 10k/39d **0.26** |

**Does it feel human?** Six bots on q2dm3 fight a recognizably player-like match: they climb
the ladders and ride the trains/lifts *mid-combat* (44 completed legs in 5 minutes), lose an
enemy around a corner and **chase it down** (89 re-acquisitions), break off when a third party
opens fire (all 10 aborts), switch to the railgun for long sightlines, and nobody ever drowns.
Deaths are high for both groups (q2dm3's lava + crossfire) — aggression, not wandering. With
the thrash fixed, `main` traded evenly with `q3` for the first time (single-run caveat: verify
at N≥5 with the aggregator). *(Plan's persona-roster showcase substituted with main-vs-q3 —
`competition --personas` isn't wired yet, a Plan 27 follow-on.)*

## 2026-07-11 — Plan 62 T2: `xon` brain + `xg` navmode join the matrix

- Driver: `acceptance matrix` gained `--navmode <m>` passthrough (the xg A/B batch runs the
  same rows against the same gates with `--navmode xg`).
- q2dm1 batch (live): **xon 2/3 swim-railgun PASS** (≥2/3 gate); **runtester+xg 2/3 PASS**.
- q2dm3/q2dm2 batches: pending operator map flips (manual q2dm3 evidence already recorded in
  `mode_perf.md` 2026-07-11: xon ride 1/4 + soak win; xg ride 2/2 + the session's only quad
  reach — both consistent with the matrix floors).
