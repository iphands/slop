# Jev language lab — what the decision model reads, ignores and misreads

> Plan 73's findings file: **what wording and which game facts make `h2oai/h2o-lightning-4b` on
> `/v1/systemone` answer qbots' questions well.** The model is frozen and deterministic; every
> line here is something *we* learned about talking to it. Numbers come from
> `cargo run -p qbots -- jev-probe lab` on the human-locked corpus
> (`context/fixtures/jev/calibration/`, 138 states, labels committed before any scoring);
> raw per-case results live in `context/fixtures/jev/lab/round*_*.jsonl`.
> Read before writing or changing a jev question, a state field, or the renderer.

**Metric key** (per question, on a corpus half): **RC** right-and-confident (top = expected label
and margin ≥ threshold) — the number to raise; **CW** confident-wrong — must stay 0; **acc**
top = expected, ignoring confidence; **mC** mean margin of correct obvious answers; **leakA**
share of ambiguous cases answered confidently with an *acting* label (not posture's no-op
`hold`); **flatA** most confident acting answer to an information-free state; **sep** mC − mean
ambiguous margin. Thresholds are the provisional 0.50 (binary posture) / 0.30 (3-way objective).

## Round 1 — 2026-11-06, tuning half, 10 single-change variants

| variant | posture RC / CW / acc / mC / leakA / flatA | objective RC / CW / acc / mC / leakA / flatA |
|---|---|---|
| baseline | 0% / 0% / 82% / 0.229 / 0% / 0.507 | 75% / 0% / 100% / 0.457 / 60% / 0.637 |
| drop-dmg | 0 / 0 / 82 / 0.253 / 0 / 0.507 | 75 / 0 / 100 / 0.427 / 50 / 0.637 |
| drop-loc | 0 / 0 / 82 / 0.287 / 0 / 0.507 | 75 / 0 / 100 / 0.450 / 60 / 0.637 |
| drop-map | 0 / 0 / 91 / 0.208 / 0 / 0.507 | **8 / 8 / 50 / 0.180** / 40 / 0.637 |
| add-threat | 0 / 0 / **100** / 0.300 / 0 / **0.000** | 75 / 0 / 100 / 0.484 / 80 / 0.418 |
| hp-band-only | 0 / 0 / 82 / 0.273 / 0 / 0.507 | 75 / 0 / 92 / 0.464 / 80 / 0.637 |
| **add-summary** | **73 / 0 / 100 / 0.545** / 29 / **0.000** | **100 / 0 / 100 / 0.676** / 70 / 0.398 |
| options-consequence | 0 / 0 / **64** / 0.185 / 0 / 0.328 | 58 / 0 / 92 / 0.480 / 50 / 0.608 |
| instruction-criterion | 0 / 0 / 82 / 0.261 / 0 / 0.394 | 83 / 0 / 100 / 0.626 / 70 / 0.756 |
| instruction-role | 0 / 0 / 82 / **0.133** / 0 / 0.061 | 50 / 0 / 100 / 0.421 / 70 / 0.502 |

Findings:

1. **The model is usually right but timid on posture.** Baseline accuracy 82 %, but the mean margin
   of a *correct* answer is 0.23 — not one obvious case clears 0.50. The coded state alone does not
   make it commit. Its two wrong answers are 4 enemies near at 19 hp and 2 near at 7 hp with a
   blaster: it said `hold` on states any player calls a retreat.
2. **One plain-English sentence beats every coded fact.** `add-summary` ("summary: badly hurt,
   weak weapon, enemies close") is the only variant that moves posture off 0 % RC (→ 73 %, 100 %
   accuracy, margin 0.23 → 0.55) and takes objective to 100 % RC. The same codes, said in words,
   are what it reads. (Matches the 2026-11-07 probe: "cannot win this fight" → 0.880.)
3. **`wants:` carries the objective question.** Dropping the `map` line (which holds `wants:`)
   collapses objective accuracy 100 % → 50 % and produces the only confident-wrong answers in the
   round. The derived loadout-gap word is load-bearing; the raw numbers are not enough.
4. ~~**The model misreads `items health:none`** as "this bot has no health", not "no health pickup
   in sight".~~ **Cause retracted by round 2** (finding R2-1): renaming the line did not remove the
   pull. The observation stands — an items-only blank state draws confident acting answers — but
   the mechanism is "health mentioned on an empty state ⇒ `seek_health`", not the key name.
   Original text kept for the record: The information-free state `items health:none armor:none weapon:none powerup:none`
   draws the most confident acting answers of any blank state: objective `seek_health` at 0.637,
   posture `disengage` at 0.507; the other items-only blanks also pull `seek_health` (0.531,
   0.499). A key that names a thing the bot *has* must not be reused for a thing it *sees*.
5. **"Badly hurt" for 25–49 hp overstates.** Every confident `disengage` on an ambiguous case under
   `add-summary` is a `lo` (25–49 hp) state — the summary calls `crit` and `lo` both "badly hurt".
   The model takes the words at face value, so the words must be graded as carefully as the bands.
6. **Things that do not help:** consequence-stating option descriptions *hurt* posture (accuracy
   82 % → 64 %) — longer options dilute the choice; a criterion sentence in the instruction changes
   nothing on posture; role framing ("you are an expert coach") **halves** posture margins and
   lowers objective RC to 50 %. Dropping `dmg`/`loc` or the hp number changes almost nothing —
   the model is not reading them.
7. **An explicit threat word removes flat disengages.** `add-threat` gives posture 100 % accuracy
   and no confident acting answer on any blank state, though its margins (0.30) stay below the bar.

Predictions vs results (tracker *Rounds* L1–L10): right on L1 (baseline weak), L3 (loc no-op),
L4 (drop-map hurts objective — far more than expected), L6 (hp band no-op), L7 (summary largest
gain). Wrong on L2 (dropping damage did not hurt), L5 (threat raised accuracy, not confidence),
L8 (consequence options *hurt*), L9 (criterion did nothing on posture), L10 (role framing *hurt*).

## Round 2 — 2026-11-06, tuning half, fixes for round 1's two problems + two combinations

| variant | posture RC / CW / acc / mC / leakA / flatA / sep | objective RC / CW / acc / mC / leakA / flatA / sep |
|---|---|---|
| items-renamed | 0% / 0% / **64%** / 0.352 / 0% / 0.256 / 0.134 | 67 / 0 / **75** / 0.613 / 70 / **0.724** / 0.105 |
| **summary-graded** | **82 / 0 / 100 / 0.519 / 0 / 0.000** / 0.287 | **100 / 0 / 100 / 0.664** / 60 / 0.527 / 0.233 |
| summary-graded + items-renamed | 45 / 0 / 100 / 0.483 / 0 / 0.000 / 0.189 | 75 / 0 / 83 / 0.703 / 70 / 0.527 / 0.190 |
| **summary-graded + threat** | **82 / 0 / 100 / 0.563 / 0 / 0.000 / 0.355** | 100 / 0 / 100 / 0.667 / 70 / 0.585 / 0.212 |

Findings:

- **R2-1 Renaming the items line is harmful, and finding 4's cause was wrong.**
  `pickups health_pack:not_seen …` lowers accuracy on both questions (posture 82 → 64 %, objective
  100 → 75 %) and *raises* the objective's most confident blank-state answer (0.637 → 0.724, still
  `seek_health`). Keep the short `items` keys; the model reads them fine on real states.
- **R2-2 Grading the hurt words fixes the summary's leak and makes it *more* accurate.**
  "badly hurt" (< 25 hp) / "hurt" (25–49) instead of "badly hurt" for both: posture RC 73 → 82 %,
  confident acting answers on ambiguous states 29 % → 0 %, none on blank states. Words have to be
  as calibrated as the bands they stand for.
- **R2-3 Threat + graded summary is the best posture wording so far**: RC 82 %, mean correct margin
  0.563, separation 0.355 (baseline 0.130), zero acting leakage, zero acting blank answers.
- **R2-4 The objective question has no harmless default.** Under every summary variant the blank
  states now answer `hold_position` confidently (0.40–0.59). Posture's `hold` means "leave it to
  q3" and costs nothing; the objective's `hold_position` is a real action (the bot camps). Its
  confident `seek_armor` on some *ambiguous* states (e.g. 70 hp, 0 armor, both pickups far → 0.78)
  may also mean those labels are too cautious — a question for the human, not something the lab
  flips (plan risk 2).

## Round 3 — 2026-11-06, tuning half, objective only: a harmless default option

| variant | objective RC / CW / acc / mC / leakA / flatA / sep |
|---|---|
| summary-graded (round 2, for comparison) | 100% / 0% / 100% / 0.664 / 60% / 0.527 / 0.233 |
| summary-graded + `carry_on` 4th option | 75 / **17** / 75 / **0.768** / **40** / **0.271** / **0.356** |

Answers by kind: obvious `seek_health` 5/5 and `seek_armor` 4/4 correct; **obvious `hold_position`
3/3 → `carry_on`**; blank states 10/12 → `carry_on`; ambiguous states spread over all four.

- **R3-1 A "keep roaming" option gives the objective the harmless default posture already has.**
  Blank states stop drawing confident actions (flatA 0.527 → 0.271), acting leakage on ambiguous
  states falls 60 → 40 %, and separation is the best of any objective wording (0.356).
- **R3-2 Without a place-specific reason, `hold_position` and `carry_on` are the same answer to the
  model.** Every "full health, nothing to pick up" case moved to `carry_on`; that is the whole RC
  drop and the 17 % "confident-wrong" — answers that are arguably right and cost nothing. Holding
  an area only means something once the state names an area worth holding (Plan 72's region
  layer, T14).
- **Implication for Plan 72 T10 (not yet adopted):** ask the objective as
  `seek_health / seek_armor / carry_on` — three options, so Plan 72's ≤ 3 cap still holds, with
  `carry_on` as the no-op that lets the gate's leakage stop mattering — and bring area control back
  as its own question with region facts. Adopting it means relabelling the corpus's obvious
  `hold_position` cases, which is the human's call (labels are never flipped by the lab).

## Round 4 — 2026-11-06, tuning half: the production candidate → **adopted**

State = coded fields + `threat <none|low|medium|high>` + graded summary sentence, **one state for
both questions** (a request carries one state); posture `disengage / hold`; objective
`seek_health / seek_armor / carry_on`.

| question | RC | CW | acc | mC | leakA | flatA | sep | blank states |
|---|---|---|---|---|---|---|---|---|
| posture | 82% | 0% | 100% | 0.563 | 0% | 0.000 | 0.355 | 12/12 → `hold` |
| objective | 100% | 0% | 100% | 0.712 | 40% | 0.000 | 0.212 | 12/12 → `carry_on` |

- **R4-1 The two harmless defaults make blank states free**: every information-free state answers
  the question's no-op. What remains to calibrate is ambiguous-case leakage on the objective (40 %
  at the provisional 0.30) — a threshold question for the holdout `calibrate`, not a wording one.
- **R4-2 Rules for writing a jev question (the summary of rounds 1–4):**
  1. say the situation in one short plain-English sentence, graded as finely as the bands;
  2. keep the derived one-word facts the answer hinges on (`wants:`, `threat`);
  3. give every question a harmless "leave it to q3" option;
  4. keep option descriptions short and literal — no consequences, no role play;
  5. never rename a field to "fix" a misreading without measuring it — the model reads short
     codes fine on real states.

## Arena A0 — 2026-11-06, first live run (a smoke, **not pre-registered**)

`competition --brains q3,jev_static,jev --navmodes as --count 3` on `noir.lan` q2dm1, ~4 min,
posture live (threshold 0.30), production wording. Hub: 69 asked, 69 answered and applied,
gate-reject 5.8 %, 0 transport/malformed, p ≈ 15 ms per answer, 107 asks paced out.

| arm | kills | deaths | K/D | env deaths | weapons picked |
|---|---|---|---|---|---|
| q3 | 6 | 7 | 0.86 | 1 | 3 |
| jev_static | 4 | 6 | 0.67 | 1 | 3 |
| **jev** | 4 | 10 | **0.40** | **5** (4 drown, 1 fall) | 2 |

Answers: **disengage 64 / 69** (31 on contact, 27 after a hit, 11 refresh); hold 5.

- **A0-1 The live state mix is not the corpus's mix.** Bots respawn with a blaster, so the
  common live state is "healthy, weak weapon, enemies close" — which the model, reasonably, reads
  as "break off". Disengaging from nearly every fight means fewer fights, few weapon pickups and
  more retreating. The corpus measured *whether answers are right per situation*; it never
  measured *how often each situation occurs in play*. Next: the decision journal (Plan 73 T5) to
  see the states behind the disengages and what happened after each.
- **A0-2 q3's retreat is hazard-blind** (`retreat_goal`: "step directly away from the enemy",
  256 u) — on q2dm1 that walks into water; jev, retreating far more often, drowned 4×. A q3
  movement bug that `disengage` amplifies, not a wording problem — fix it in q3 (all arms gain).
- One run of 9 bots for 4 minutes: the K/D gap is not evidence by itself (plan guard rail 5).

## Arena A1 — 2026-11-06, baseline, 3 × 10 min (3 q3 + 2 jev + 1 shadow `jev_static`)

**Death outcomes invalid** (journal bug — `died` was never set; fixed, re-run as A1b). Answer mix,
q3 agreement and hp/frag deltas are valid. 493 posture decisions journalled (326 jev, 167 shadow);
0 transport errors, 0 admission drops, live gate-reject (`no_signal`) **31–47 %** per run.

| run | jev K/D | q3 K/D | jev_static K/D |
|---|---|---|---|
| 1 | 1.18 | 0.59 | 0.67 |
| 2 | 1.38 | 0.57 | 1.00 |
| 3 | **0.18** | 0.69 | 1.29 |

- **A1-1 The model mostly agrees with q3.** Of jev's `disengage` answers, q3's own heuristic wanted
  to retreat in 162 of 232 — the respawn blaster gives q3 an aggression of ~0 too. jev's posture
  changes behaviour only in the minority where q3 wanted to fight (70 cases).
- **A1-2 "Weak weapon" carries the disengages**: 198 of 232 jev `disengage` answers have
  `weak weapon` in the summary.
- **A1-3 Live states are much less decisive than the corpus**: 31–47 % of live answers fall
  under the 0.30 margin (the corpus's holdout: 0 % of obvious posture cases did).
- **A1-4 Frags are noise at this size**: jev's K/D went 1.18 → 1.38 → 0.18 on identical code.

## Arena A1b — 2026-11-06, baseline re-run with deaths recorded, 3 × 10 min

502 posture decisions (338 jev, 164 shadow). K/D per run: jev 1.11 / 0.88 / 0.77, q3 0.68 /
0.85 / 0.67, jev_static 0.60 / 0.25 / 0.67. Live gate-reject 24–35 %. Report:
`context/fixtures/jev/arena/a1b_report.txt`.

| cell | n | died in window | hp Δ |
|---|---|---|---|
| jev disengage, applied (all) | 204 | 14.2 % | −4.0 |
| jev disengage \| q3 wanted **fight** (jev overrode q3 and retreated) | 83 | 3.6 % | −9.0 |
| shadow disengage \| q3 wanted **fight** (q3 fought; model ignored) | 42 | 0.0 % | −5.6 |
| jev disengage \| q3 wanted retreat (they agreed) | 171 | 15.8 % | −0.9 |

- **A1b-1 jev's only real effect is pulling healthy bots with a weak weapon out of fights.** All
  71 applied overrides are "healthy, weak weapon, enemies close / at a distance". Against the
  shadow's same cell they lost more health and died no less — the override does not pay
  (low power: a hypothesis, tested in A2).
- **A1b-2 Deaths follow the state, not the answer**: the hopeless states (badly hurt, threat high)
  carry the deaths whatever the model said; confident disengages die more (17 % at margin ≥ 0.50)
  because they are those states.

## Arena A2 — 2026-11-07, one change: a disengage counts only when the bot is hurt, 3 × 10 min

`jev.disengage_needs_hurt: true` (hub-side; wording and threshold unchanged; the shadow follows
the same rule). 515 decisions. K/D over the round: jev 0.72, q3 0.65, jev_static 1.08.
Report: `context/fixtures/jev/arena/a2_report.txt` (its "no_signal" rows include the 141
`suppressed` answers).

- Overrides (disengage applied while q3 wanted to fight): **83 → 0**; applied disengages
  204 → 108; died after an applied disengage **22 %** (what remains is the hurt states).
- **A2-1 Retreat vs fight in the one case jev changes, side by side** (healthy, weak weapon, model
  says disengage, q3 wants to fight): A1b — jev **retreated**, n = 83, died 3.6 %, hp −9.0;
  A2 — jev **fought**, n = 90, died 0.0 %, hp −12.9. Fighting costs a little more health,
  retreating a few more deaths: **neither clearly wins**.

## Arena conclusion (Plan 73 T8, 9 × 10 min, 1 510 journalled decisions)

1. **The posture question mostly agrees with q3's own heuristic** (q3 wanted to retreat on
   ~67 % of the model's disengages); its one systematic disagreement — pull healthy bots with
   the respawn weapon out of fights — is **roughly neutral** in outcome.
2. **Live states are far less decisive than any hand-written corpus**: 24–47 % of live answers
   fall under the calibrated 0.30 margin; the corpus holdout had none.
3. **Frags cannot judge this at this scale**: one arm's K/D ranged 0.18–1.38 across runs of
   identical code; outcome windows per decision are the usable signal.
4. **What to try next is a different question, not a different gate**: one whose answer q3 does
   *not* already compute — e.g. which pickup to go for (the objective question, now that arena
   journals can judge its labels), or when to re-engage after a retreat.

## Round 5 — 2026-11-07, Plan 74: the pickup objective (`objective_v2`, out-of-sight pads)

Question `seek_weapon / seek_health / seek_armor / carry_on`, offering only the classes the
state's `pads` line names (travel-cost band to the best reachable, not-known-empty pad). Corpus
`context/fixtures/jev/calibration/objective_v2/` (78 states, no enemy in view). Tuning half, all
15 variants; raw: `context/fixtures/jev/lab/round5_objective_v2_{tuning,holdout}.jsonl`.

| variant (tuning) | RC | CW | acc | leakA | flatA |
|---|---|---|---|---|---|
| baseline (fields only) | 62% | 0% | 92% | 53% | 0.466 |
| summary-graded | 88% | 0% | 100% | 53% | 0.139 |
| **summary-graded-threat (production)** | **96%** | **0%** | 96% | 67% | 0.187 |
| drop-map (loses `wants:`) | 31% | **19%** | 42% | 0% | 0.689 |
| instruction-criterion | 77% | 0% | 92% | 67% | 0.788 |

Holdout (once, production): **RC 100 %, CW 0 %, acc 100 %, flatA 0.015**, leakA 64 %.

- **R5-1 The production state wording carries over unchanged.** The graded summary + threat that
  won posture also wins the pickup objective; no new wording was needed.
- **R5-2 `wants:` is load-bearing for objectives** — dropping the `map` line (which carries it)
  is the only variant with confident-wrong answers (19 %).
- **R5-3 "Ambiguous leakage" is not harm for a pickup question.** Every ambiguous answer was a
  defensible pickup (hurt → health, weak → weapon, no armor → armor); `calibrate`'s ≤ 10 %
  ambiguous criterion (built for posture, where acting changes q3) rejects it anyway. Decision:
  W2 for this question = holdout RC ≥ 80 %, CW 0, flat answers below the 0.30 gate — met. The
  arena, not the corpus, judges whether the choices are good.
