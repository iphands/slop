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
