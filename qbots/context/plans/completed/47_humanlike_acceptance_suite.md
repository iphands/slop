# Plan 47 — Human-like play acceptance suite (the capstone gate)

> **Status**: pending
> **Created**: 2026-07-09
> **Depends on**: Plans 35, 42, 43, 46 (traversal), 27–33 (behavior), 44 (zb2, optional rows)
> **Goal**: One repeatable, scripted acceptance run that *proves* the series goal — bots that navigate whole maps (ladders, water, platforms, lifts), collect what they need, and fight naturally — with recorded behavior metrics and a written baseline, so future changes have a regression gate.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Each capability landed with its own one-off proof (mode_perf tables, brain_notes
sections). Nothing runs them *together*, per brain, on demand. Build a `just acceptance`
(or `qbots acceptance`) driver over the existing scenario + competition CLIs, add the few
behavior counters the logs don't yet expose, and record the baseline in
`context/acceptance.md`.

**Deliverables**:
1. **Traversal matrix** (scripted): per brain (`main`, `q3`, `runtester`, later `zb2`):
   - q2dm1 `spawn-to-weapon railgun` (swim) — reached
   - q2dm3 `spawn-to-weapon railgun --instance 1 --count 4` (train + lift) — ≥ 3/4
   - q2dm3 `spawn-to-item quaddamage --count 4` (ladder + train over lava) — ≥ 3/4 (Plan 35)
   - q2dm2 `spawn-to-spawn --count 8` — 8/8 within cap
2. **Behavior counters** in competition output (extend `FleetStats`/per-bot logs):
   weapon switches (with engagement range at switch), health/armor pickups while hurt,
   chases initiated after LOS loss (and % converted to kills), lift/train rides completed,
   swims completed, drownings (must be 0), third-party breaks.
3. **`context/acceptance.md`** — the recorded baseline table + how to re-run; a documented
   pass/fail contract per row.
4. A 5-minute mixed showcase (`main` persona roster vs `q3` roster on q2dm3) demonstrating
   natural play end-to-end, summarized in `brain_notes.md`.

**Estimated effort**: Medium (1 day).

## Context

- Everything runs on existing binaries: scenarios (`spawn-to-*`, exit code 0/2 per
  `AGENTS.md` §Movement Testing), `competition` (FleetStats scoreboard), `generate-map-cache`.
  The driver is glue — a `justfile` recipe or a small `tools/` binary (Constraint: **no
  tmp scripts**; per AGENTS.md §Tooling it must live in `justfile` or `crates/tools/`).
- Counters: the recorder already flags `S`/`P`(/`L` after Plan 46) frames and SUMMARY
  lines; competition logs have obituaries. Missing counters (switch-at-range, hurt-pickup,
  chase-conversion, third-party) get emitted where the events already exist
  (`combat.rs` switch requests, `items` pickups + health state, Plan 29's chase/break logs).
- Pass thresholds start at the values already proven in `mode_perf.md` (railgun ≥ 3/4,
  swim reached, etc.) and tighten as Plan 35 lands (quad from far spawns ≥ 3/4).

## Step-by-Step Tasks

### T1: Behavior counters

**Files**: `crates/brain/src/combat.rs`, `items.rs`, `brains/main.rs`, `crates/qbots/src/`
(FleetStats aggregation)

**What to do**: Emit structured one-line events (existing per-bot log stream):
`EVT switch weapon=<w> dist=<d>`, `EVT pickup class=<c> health=<h>`, `EVT chase start|convert|abort`,
`EVT third_party`, `EVT ride done kind=train|lift|ladder`, `EVT swim done`, `EVT drown`.
Aggregate per bot at run end (extend the FleetStats summary). Keep events cheap + greppable.

### T2: Acceptance driver

**Files**: `justfile` (recipe) or `crates/tools/src/bin/acceptance.rs`

**What to do**: Script the traversal matrix + two 5-min competitions; parse SUMMARY lines
and EVT aggregates; print a single pass/fail table (row = check, columns = brain).
Parameters: `--addr`, `--brains`, `--rows` filter. Document server prerequisites per row
(which map must be loaded — the driver runs per-map batches and tells the operator when to
switch maps, or drives qctrl/rcon if available; keep manual-switch as the baseline).

### T3: Record the baseline

**Files**: `context/acceptance.md` (new), `context/brain_notes.md`

**What to do**: Run the full suite; record the table with date, commit hash, and per-row
result; define the regression contract (a future PR that drops a green row to red must fix
or explicitly re-baseline). Append `brain_notes.md`.

### T4: Showcase run

**What to do**: q2dm3, 5 min, `main` persona roster (Plan 27) vs `q3` chars; capture the
scoreboard + counter table; write the "does it feel human" narrative paragraph (chases,
heals, weapon choices, rides observed) in `acceptance.md`. This is the series-goal
demonstration for the user.

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/qbots/src/` + `crates/brain/src/*` | EVT counters + aggregation | P0 |
| `justfile` / `crates/tools/src/bin/acceptance.rs` | driver | P0 |
| `context/acceptance.md` | baseline + contract | P0 |

## Open Questions / Risks

1. **Map switching mid-suite** needs the server changed (qctrl/rcon or manual).
   *Mitigation*: per-map batches with operator prompts; qctrl integration optional.
2. **Flaky rows** (spawn variance — railgun 1–4/4 observed). *Mitigation*: thresholds set
   at proven floors (≥ 3/4 with one retry), not maxima; the driver supports `--retries 1`.
3. **Counter noise** (what counts as "a chase"?). *Mitigation*: counters are defined by
   the emitting plan's events (Plan 29 owns chase semantics); acceptance only aggregates.

## Verification Checklist

- [ ] T1: EVT events emitted + aggregated (unit test the parser); commit.
- [ ] T2: one command runs the matrix and prints the table; commit.
- [ ] T3: `context/acceptance.md` baseline recorded (date + commit); commit.
- [ ] T4: showcase narrative + tables written; `brain_notes.md` appended; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
