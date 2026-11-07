# Plan 73 — Jev lab + arena: learn which situations, facts and wording make the model decide well

> **Status**: in-progress
> **Created**: 2026-11-06
> **Depends on**: Plan 72 T9 (`qbots jev-probe`) for Wave A; Plan 72 T10 + T11 (the first real
> question + telemetry) for Wave B. Plan 72 is the thing being measured; this plan is its
> testing ground and produces the evidence for Plan 72's W2, W3 and T16 gates.
> **Goal**: Build a repeatable loop — a fast offline **language lab** and a live **arena** of
> q3 vs jev bots that journals every decision with what happened next — and use it to learn,
> and write down, which game facts and wording make the decision model give good answers and
> which make it give mush.
> **Agent**: implementation agent (ralph-loop) for T2, T4, T5, T6, T7; **T1 is human** (label
> approval); **T3 and T8 are live** — agent-run only with the user's explicit permission to
> call the endpoint / run the Q2 server, otherwise human-run.

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.
> Rule A (zero warnings) and Rule B (commit per task) are not optional here.

---

## TL;DR

**What**: The decision model is frozen and deterministic — it never learns. **We do.** This plan
builds the instruments to learn *what to say to it*: a lab that scores many phrasings of the same
labelled situations in minutes, and an arena where real bots act on its answers and every
decision is journalled with its outcome. What we learn is written down and ported into Plan 72's
production renderer and questions.

**Deliverables**:
1. A **scenario corpus** per question (posture, objective), human-locked, split
   deterministically into a **tuning** half and a **holdout** half (≥10 obvious + ≥10 ambiguous
   in each half, so the holdout alone can run Plan 72's `calibrate`).
2. **`qbots jev-probe lab`** — closed, code-defined *variants* (one change each: a fact added or
   removed, numbers vs words, option wording, instruction wording), scored on the tuning half.
3. **`context/distilled/jev_lab.md`** — the findings: what wording and which facts work, what
   doesn't, with the numbers. **This is the real product of the plan.**
4. The winning wording ported into Plan 72's renderer/questions (`STATE_SCHEMA_VERSION` bump,
   goldens updated), confirmed **once** on the holdout, then Plan 72's W2 `calibrate` run on the
   holdout → frozen thresholds.
5. A **decision journal**: one JSONL line per decision (state, question, probabilities, margin,
   applied or not, and the **outcome** a fixed window later), written by the hub to `logs/jev/`.
6. **Shadow mode** (`jev.shadow: true`): `jev_static` bots ask and journal but never steer — so
   we see what the model *would* have said where q3 decided alone, without changing the control.
7. An **arena** roster + **`qbots jev-probe journal`** analysis: outcome rates per choice,
   model-agreed vs model-disagreed outcomes, confidence vs outcome, per-fact breakdowns.
8. Real arena situations harvested back into the scenario corpus (human-labelled).

**Estimated effort**: Large (3 days) of agent work — Wave A ≈ 1 day, Wave B ≈ 2 days — plus the
live rounds (each round ≈ 10 min of server time + analysis), which repeat for as long as they keep
teaching us something.

---

## Context

### Why this plan exists (the user's framing, 2026-11-06)

The first reading of "4 q3 bots vs 1 jev bot, watch it improve until it dominates" was a
tournament, and that reading has real problems (single runs are noise; a frozen model cannot
improve; jev shares q3's aim and movement). The user's actual intent: **the learner is us** —
find out which scenarios work best and worst, and what language and game facts a situation needs
for the model to answer it well. That is precisely Plan 72's own lever ("fix the state, never
loosen the cut-off"), made systematic. Live evidence that it matters (2026-11-07 probes, Plan 72
Evidence): the *same* bad situation scored disengage **0.441** as a 3-way question and **0.663**
as a binary one, and only an explicit "cannot win this fight" sentence reached **0.880**. Wording
and framing move the answer more than anything else we control.

### What the two loops are for

| | Language lab (Wave A) | Arena (Wave B) |
|---|---|---|
| Needs | the endpoint (loopback on the dev box) | endpoint + a parked Q2 server |
| Speed | ~0.1 s per ask → dozens of variants in minutes | ~10 min per round |
| Ground truth | human labels on hand-written situations | **what actually happened next** |
| Answers | which wording makes the model right *and* confident | whether right-and-confident pays off in play |
| Weakness | situations are ours, not the game's | noisy, slow, needs many decisions |

They feed each other: arena situations the model got wrong become new lab scenarios; lab-winning
wording goes back into the arena.

### Guard rails — how we keep from fooling ourselves

1. **Labels before answers.** The corpus is locked (committed) before any lab run touches it; the
   commit order is the evidence. New scenarios from the arena are labelled by a human before they
   are scored.
2. **Tuning vs holdout.** Variants are compared on the tuning half only. The holdout is scored
   **once**, for the final variant. Choosing wording on the same cases you report would just find
   phrasing that agrees with those cases.
3. **One change per variant / per arena round**, and the expected effect is written down *before*
   the run (in the tracker's Rounds table). A surprise is a finding; a silently-moved goalpost is
   not.
4. **Thresholds come only from `calibrate` on the holdout** (Plan 72 W2), never from arena
   outcomes. The arena may say "this question is useless" (delete it), never "lower the bar".
5. **Decisions before frags.** The arena's primary unit is the journalled decision with its
   outcome. K/D is secondary (this repo has seen 1.00 → 0.86 → 2.60 on identical code). A
   comparison needs **≥100 applied decisions per question per arm** before it is reported.
6. **The control stays a control.** Shadow mode asks and journals but never steers; a parity test
   proves a shadow `jev_static` brain's output equals a `NullDecider` one tick for tick.

### Lab metrics (per question, per variant, on a corpus half)

- **right-and-confident**: share of obvious cases where the top choice is the expected label and
  the margin clears the current threshold — the number we want up;
- **confident-wrong**: obvious cases answered wrongly *above* the threshold — must stay ~0;
- **accuracy** (top choice = expected, ignoring confidence) and **mean margin** on obvious cases;
- **ambiguous leakage**: share of ambiguous cases above the threshold — should stay low;
- **flat max margin**: the most confident answer to a no-information state — should stay low;
- **separation**: mean margin (obvious, correct) − mean margin (ambiguous).
A variant "wins" on right-and-confident without worsening confident-wrong or leakage.

### Variant families (closed — each is code, reviewed like the renderer)

Variants transform the **rendered** text of closed `DecisionState`s and the closed question
specs; they never take free text, so the security boundary of Plan 72 T2 holds. Families:
- **facts**: drop or add one field (e.g. no `dmg`; add an explicit `threat:` summary derived from
  enemy count × distance × our weapon; add `odds:` = our loadout vs visible enemies);
- **form**: numbers vs bands vs words (`hp:15(crit)` / `hp:crit` / `health critical`);
- **options**: label and description wording (`disengage` vs `retreat`; one-line vs
  consequence-stating descriptions);
- **instructions**: the question sentence (neutral vs role-framed vs criterion-stating).

### Decision journal (one JSONL line per decision)

`{ts, run, bot, arm (jev|shadow), map, schema, variant, ticket, trig, question, state_text,
st_hash, labels, probabilities, margin, n_eff, gated (apply|no_signal), applied (bool),
q3_own (for posture: what q3char wanted that tick), outcome{...}}`.
Outcome windows (brain time, from the tick the decision is applied or, in shadow, resolved):
- **posture** — 10 s: `died`, `hp_delta`, `armor_delta`, `frags_delta`, `damage_taken`;
- **objective** — until release or 20 s: `reached` (within 64 u), `secs_to_reach`,
  `died_en_route`, `picked_up` (pickup print during the window), `hp_delta`, `armor_delta`.
The brain measures outcomes (it has the worldview and the brain clock) and reports them through
`DecisionSource::outcome(ticket, Outcome)`; the hub joins them with what it sent and writes the
line. `logs/` is gitignored.

### The arena

Roster (`context/fixtures/jev/arena.yaml`): **3 × q3, 2 × jev, 1 × jev_static (shadow)** on a
parked q2dm1 — more jev bots than the original "1 vs 4" because the unit of learning is the
decision, and two jev bots double the decisions per minute; the shadow control adds "what the
model would have said" where q3 decided alone. Each **round** = one change (or none, for a
baseline) + a written prediction + ≥3 runs of 10 min + `jev-probe journal` analysis + a findings
entry.

### How this closes Plan 72

| Plan 72 gate | Evidence from Plan 73 |
|---|---|
| W2 (live probe + calibration) | T3/T4: the lab's sweeps, the holdout confirmation, and `calibrate` on the holdout → frozen thresholds; plus 72's own `health` / `bench` / smoke rows |
| T10/T12 question wording | T4: the winning wording, ported into the renderer and question specs |
| W3 (`applied/asked`, gate-reject, admission-drop per question) | T7/T8: arena rounds produce the journal those ratios are computed from |
| T16 acceptance A/B | T8: the arena protocol (matched outcomes, ≥100 decisions/arm) replaces a K/D-first A/B |

Plan 72 is marked **blocked** in `SERIES.md` while Wave A runs (its next step needs W2), resumes
for T10/T11, and this plan is then blocked until 72's T10/T11 land (Wave B needs them).

---

## Step-by-Step Tasks

### Wave A — the language lab (needs Plan 72 T9 only)

### T1: Lock the scenario corpus (human) and split it

**File**: `context/fixtures/jev/calibration/{posture,objective}/{obvious/<label>,ambiguous,flat}/*.txt`

**What to do**: The agent drafted 56 situations on 2026-11-06 (28 per question: 14 obvious with
an expected answer, 11 ambiguous, 3 flat). Extend to **≥24 obvious and ≥22 ambiguous per
question** (so each half holds ≥10 + ≥10 — the holdout must be able to run Plan 72's
`calibrate` alone), plus ~5 flat. The human reviews every label (move a file to change it) and
the corpus is committed **before any lab or calibrate run**. The split is deterministic and
content-independent: a file is **holdout** iff its file name's FNV-1a hash is odd.

**Verify**: `find … -name '*.txt' | wc -l` per folder meets the counts; the split helper (T2)
reports ≥10/≥10 per half per question; the corpus commit precedes every Evidence row that scores
it.
**Commit**: `task(T1): lock the jev scenario corpus (human-labelled, pre-registered)`

### T2: `jev-probe lab` — variants, split, scoring (offline)

**File**: `crates/qbots/src/jev/lab.rs` (new), `crates/qbots/src/jev/probe.rs` (new
`ProbeCmd::Lab`), `crates/qbots/src/jev/mod.rs`

**What to do**: A closed `Variant` enum (baseline + one per family entry above), each a pure
function `(rendered state text, QuestionSpec) → (text, QuestionSpec)`; corpus loader with the
FNV-1a split; scorer computing every *Lab metric*; `jev-probe lab --variant <v>|--all
[--half tuning|holdout] [--json]`. Every ask is harvested as in T9. **Holdout runs refuse** unless
`--confirm-holdout` is passed, and the tracker records each holdout use.

**Verify**: `cargo test -p qbots jev_lab` — variants are pure and deterministic (100× equal);
the baseline variant renders byte-identically to the production renderer; the split is stable
and balanced on the T1 corpus; scoring over canned answers reproduces hand-computed metrics;
`--half holdout` without `--confirm-holdout` errors.
**Commit**: `task(T2): jev-probe lab — closed variants, tuning/holdout split, scoring`

### T3: Run the lab and write down what works (live)

**File**: `context/distilled/jev_lab.md` (new), tracker Evidence + Rounds

**What to do**: Baseline on the tuning half, then one family at a time. For each variant: the
prediction first (tracker), the run, the metrics table, a one-line conclusion in
`jev_lab.md`. Keep going while variants still move right-and-confident; stop a family when two
variants in a row change nothing. `jev_lab.md` is dense house style: what the model reads, what
it ignores, what confuses it, with numbers.

**Verify**: every variant run is an Evidence row (command, variant, half, metrics); `jev_lab.md`
states, per question, the best variant, its tuning-half metrics, and at least three "doesn't
work" findings.
**Commit**: `task(T3): jev language-lab findings (tuning half)`

### T4: Port the winner, confirm on the holdout, calibrate (closes Plan 72 W2)

**File**: `crates/qbots/src/jev/render.rs`, `crates/brain/src/jev/{state,question}.rs`,
goldens, `config.example.yaml`, Plan 72 tracker (W2 row)

**What to do**: Port the winning variant into the production renderer / question specs (closed
data only), bump `STATE_SCHEMA_VERSION`, update the goldens with the new expected bytes. Run the
**holdout once** (`--confirm-holdout`); if the winner's holdout right-and-confident is within
10 points of its tuning number, accept; otherwise record the overfit and fall back to the
baseline wording. Then run Plan 72's `jev-probe calibrate` on the holdout → thresholds into
`config.example.yaml`, frozen. Record Plan 72's W2 gate.

**Verify**: goldens updated with the version bump; holdout result + calibrate output in Evidence;
Plan 72 tracker's W2 row filled.
**Commit**: `task(T4): port the lab-winning wording, holdout-confirmed; calibrate (Plan 72 W2)`

### Wave B — the arena (needs Plan 72 T10 + T11)

### T5: Decision journal + outcome windows

**File**: `crates/brain/src/jev/source.rs` (`Outcome`, `DecisionSource::outcome`),
`crates/brain/src/brains/jev/policy.rs` (window tracking), `crates/qbots/src/jev/{hub,journal}.rs`

**What to do**: Brain-side outcome tracking per *Decision journal* (brain clock; closed on death,
level change or window end); hub-side journal writer joining what was sent with the outcome,
`logs/jev/journal-<unix>.jsonl`, best effort (never blocks a bot). `jev.journal: true` to enable.

**Verify**: `cargo test -p brain jev_outcome` — scripted ticks produce the expected outcome for
each field (death inside a window, hp deltas, reached-goal, window expiry); `cargo test -p qbots
jev_journal` — a scripted hub run writes one well-formed line per decision with the outcome
joined; journal writes never happen on the tick path.
**Commit**: `task(T5): jev decision journal with outcome windows`

### T6: Shadow mode for the control arm

**File**: `crates/brain/src/brains/jev/{mod,policy}.rs`, `crates/brain/src/brains/mod.rs`,
`crates/qbots/src/{config.rs,supervisor.rs}`

**What to do**: With `jev.shadow: true`, `jev_static` bots get a hub link, ask on the same
triggers, resolve and journal (`arm=shadow`, plus q3's own posture choice that tick), and
**never** feed the decision to q3.

**Verify**: a **parity test** — a shadow `jev_static` and a `NullDecider` `jev_static` produce
identical `BrainOutput` over the T6 (Plan 72) golden script while the shadow one receives
scripted decisions; journal lines carry `arm=shadow` and `applied=false`.
**Commit**: `task(T6): shadow mode — the control asks and journals, never steers`

### T7: Arena roster + `jev-probe journal` analysis (offline)

**File**: `context/fixtures/jev/arena.yaml`, `crates/qbots/src/jev/probe.rs`
(`ProbeCmd::Journal`), `crates/qbots/src/jev/journal.rs`

**What to do**: The roster (3 q3, 2 jev, 1 jev_static shadow). `jev-probe journal <files…>`
reports, per question: applied/asked, outcome rates per choice, **agreed vs disagreed**
(jev: applied vs gated; shadow: model's choice vs q3's own), outcome vs margin (are confident
answers better?), per-fact breakdowns (which state lines distinguish good from bad outcomes),
and lists the worst-outcome decisions as **candidate scenarios** for the corpus (state text ready
to label). Refuses to compare arms with <100 applied decisions per question.

**Verify**: `cargo test -p qbots jev_journal_report` over a synthetic journal with known answers;
the roster parses (`roster::specs_from`) and fits the netname budget.
**Commit**: `task(T7): arena roster and jev-probe journal analysis`

### T8: Arena rounds (live) — learn, feed back

**File**: tracker Rounds table, `context/distilled/jev_lab.md`, corpus candidates

**What to do**: Round 0 = baseline (no change). Each later round: one change + a written
prediction → ≥3 × 10-min runs → `jev-probe journal` → a findings entry. The worst-outcome
decisions are offered to the human as new labelled scenarios (T1 rules); lab re-runs on them.
Continue while rounds keep teaching; stop rule: three consecutive rounds with no finding.

**Verify**: Rounds table rows with prediction / result / conclusion; `jev_lab.md` gains an
"arena" section; at least one arena-derived scenario batch labelled and committed.
**Commit**: `task(T8): arena rounds — findings and harvested scenarios` (one commit per round)

### T9: Hand the evidence to Plan 72 (W3, T16) and close

**File**: Plan 72 tracker (W3 gate row, T16 Evidence), `context/pitfalls.md`,
`context/brain_notes.md`, `SERIES.md`

**What to do**: Compute Plan 72's W3 ratios from the arena journals; feed T16's acceptance with
the arena protocol's matched outcomes; pitfalls for anything that took more than one attempt.

**Verify**: Plan 72's W3 row filled from journals; this plan's checklist complete.
**Commit**: `task(T9): hand arena evidence to Plan 72 W3/T16; close Plan 73`

---

## Dependency matrix

```text
Plan 72 T9 (done) ─▶ T1 (human) ─▶ T2 ─▶ T3 (live) ─▶ T4 ─▶ [Plan 72 W2 recorded]
                                                              │
                                              Plan 72 T10, T11 ◀┘
                                                              │
                                    T5 ─▶ T6 ─▶ T7 ─▶ T8 (live, repeating) ─▶ T9
```

- **T2 needs only Plan 72 T9's probe** and can be built while the human locks T1.
- **T5/T6 need Plan 72 T10** (the first real question) and **T11** (telemetry the journal joins).
- Status flips (RULES C.5): Plan 72 `blocked` while T1–T4 run; this plan `blocked` while Plan 72
  does T10/T11; then this plan resumes at T5.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `context/fixtures/jev/calibration/**` | the human-locked corpus (tuning/holdout) | P0 |
| `crates/qbots/src/jev/lab.rs` | variants, split, scoring | P0 |
| `context/distilled/jev_lab.md` | **the findings** — what language and facts work | P0 |
| `crates/qbots/src/jev/render.rs`, `crates/brain/src/jev/{state,question}.rs` | the ported winner (schema bump) | P0 |
| `crates/brain/src/brains/jev/policy.rs` | outcome windows; shadow mode | P1 |
| `crates/qbots/src/jev/{hub,journal}.rs` | journal writer + analysis | P1 |
| `crates/qbots/src/jev/probe.rs` | `lab`, `journal` subcommands | P1 |
| `context/fixtures/jev/arena.yaml` | the arena roster | P2 |

## Open Questions / Risks

1. **Overfitting the wording to our own scenarios.** *Mitigation*: tuning/holdout split, holdout
   used once, arena outcomes as an independent check, arena-derived scenarios added over time.
2. **Our labels may be wrong.** An "obvious" case can be wrong-headed. *Mitigation*: the arena's
   outcomes are the second opinion — a label the arena keeps contradicting is re-reviewed by the
   human (recorded, never silently flipped).
3. **Outcome windows are a crude proxy for "good decision".** Dying 8 s after holding a fight may
   be bad luck. *Mitigation*: ≥100 decisions per comparison, agreed-vs-disagreed contrasts (luck
   hits both sides), and treating single-decision stories as hypotheses for the lab, not results.
4. **Shadow asks add hub load and could crowd the jev bots.** *Mitigation*: the 6-bot arena is far
   under budget (≤6/5 s worst case ≈ 1.2 req/s); the journal records admission drops by arm.
5. **Variants could smuggle free text into prompts.** *Mitigation*: variants are closed code
   transforms over rendered closed data, reviewed like the renderer; no variant reads a file.
6. **Two plans interleaving** is a process risk. *Mitigation*: SERIES records which one is blocked
   on which and why at every switch; each plan's tracker names the other's task it waits on.
7. **Endpoint permission.** Wave A's live runs (T3, T4) call `cosmo.lan:8710` (loopback on the dev
   box). Asked on 2026-11-06; the agent runs them only once the user says yes, otherwise they are
   human-run with `cargo run -p qbots -- jev-probe lab …`.

## Verification Checklist

- [ ] T1: corpus ≥24 obvious + ≥22 ambiguous + ~5 flat per question, human-reviewed, committed before any scoring; each half ≥10/≥10
- [x] T2: variants pure/deterministic; baseline ≡ production renderer; split stable; scorer matches hand-computed metrics; holdout guarded
- [ ] T3: every variant run in Evidence with its prediction; `jev_lab.md` has per-question best variant + ≥3 "doesn't work" findings
- [ ] T4: winner ported with schema bump + goldens; holdout scored once; calibrate on holdout → frozen thresholds; Plan 72 W2 row filled
- [ ] T5: outcome-window tests per field; journal line per decision with outcome; nothing on the tick path
- [ ] T6: shadow `jev_static` ≡ `NullDecider` `jev_static` output (parity); journal `arm=shadow`, `applied=false`
- [ ] T7: journal report over a synthetic journal matches expected; <100-decision comparisons refused; roster parses
- [ ] T8: Rounds table with predictions; arena section in `jev_lab.md`; ≥1 arena-derived scenario batch committed
- [ ] T9: Plan 72 W3 filled from journals; pitfalls + brain_notes; plan + tracker moved to `completed/`
- [ ] Rule A held at every commit: `cargo fmt` + `cargo clippy -- -D warnings` + `cargo test` green
- [ ] Rule C: plan + tracker `git mv` to `completed/` and `SERIES.md` marked done
