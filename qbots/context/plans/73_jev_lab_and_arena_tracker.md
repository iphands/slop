# Plan 73 — Jev lab + arena — Tracker

## Overview

- Status: 11% complete (1/9 tasks)
- Start date: 2026-11-06
- Plan: `context/plans/73_jev_lab_and_arena.md`
- Purpose: the learner is **us** — find which situations, facts and wording make the frozen
  decision model answer well, and write it down (`context/distilled/jev_lab.md`).
- Interleaves with Plan 72: Wave A (T1–T4) produces Plan 72's W2 evidence and its question
  wording; Wave B (T5–T9) needs Plan 72 T10 + T11 and produces Plan 72's W3/T16 evidence.

## Resume Instructions

1. Read `AGENTS.md`, `../CLAUDE.md`, `context/plans/RULES.md` **in full**, this plan, Plan 72's
   plan + tracker (the thing being measured), and `context/distilled/jev_lab.md` once it exists.
2. **Guard rails are not optional** (plan *Guard rails*): labels before answers; tuning half for
   choosing, holdout **once**; one change per variant/round with the prediction written first;
   thresholds only from `calibrate` on the holdout; decisions before frags (≥100 per comparison);
   shadow never steers.
3. **Live work needs permission.** T3/T4 call the endpoint; T8 runs the Q2 server. The agent does
   these only after the user says so in-session; otherwise they are human-run.
4. Statuses are RULES' set only (`pending` | `in-progress` | `done` | `blocked` | `skipped`).
   When switching between Plans 72 and 73, mark the waiting one `blocked` in `SERIES.md` with the
   task it waits on.
5. Commit per task as `task(TN): …`, with this tracker's row in the same commit; one commit per
   arena round in T8.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Lock the scenario corpus (human), split | `context/fixtures/jev/calibration/**` | pending | first 56 committed `34f28aefa` at the user's request, **before any scoring**; split report shows every half short (5–9 per category vs ≥10) and both flat sets entirely in tuning — needs ≈ +10 obvious, +11 ambiguous, +3 flat per question |
| 2 | T2: `jev-probe lab` — variants, split, scoring | `crates/qbots/src/jev/lab.rs`, `probe.rs` | done | 10 closed variants (baseline + 3 drop-a-fact, add-threat, hp-band-only, add-summary, consequence options, criterion / role instructions); FNV-1a split; holdout needs `--confirm-holdout`; `--split-report` is offline. 7 tests |
| 3 | T3: Run the lab, write findings | `context/distilled/jev_lab.md` | pending | **live** — needs endpoint permission |
| 4 | T4: Port winner, holdout once, calibrate | renderer, question specs, goldens | pending | closes Plan 72 W2 |
| 5 | T5: Decision journal + outcome windows | `brain::jev::source`, `JevPolicy`, `qbots::jev::journal` | pending | needs Plan 72 T10 + T11 |
| 6 | T6: Shadow mode for `jev_static` | `brains/jev`, config, supervisor | pending | parity test: shadow ≡ NullDecider output |
| 7 | T7: Arena roster + `jev-probe journal` | `arena.yaml`, `journal.rs`, `probe.rs` | pending | offline |
| 8 | T8: Arena rounds (live, repeating) | Rounds table, `jev_lab.md` | pending | **live** — needs Q2 server + permission |
| 9 | T9: Hand evidence to Plan 72; close | Plan 72 tracker, pitfalls, brain_notes | pending | |

## Rounds (lab variants and arena rounds)

Every row is written **before** its run (prediction), then completed.

| # | Date | Loop | Change (one) | Prediction | Result | Conclusion |
|---|------|------|--------------|------------|--------|------------|

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-06 | `cargo run -p qbots -- jev-probe lab --split-report` (offline, corpus @ `34f28aefa`) | posture: tuning 6 obv / 5 amb / 3 flat, holdout 8 / 6 / 0; objective: tuning 9 / 5 / 3, holdout 5 / 6 / 0 | 1 | no model call; confirms T1 must grow the corpus |
| 2026-11-07 | Plan 72 review probes (carried over as motivation) | same bad situation: disengage 0.441 (3-way) vs 0.663 (binary); explicit "cannot win" → 0.880 | 3 | framing moves the answer more than anything else we control |

## Notes / Deviations

- Origin (2026-11-06): the user's "4 q3 vs 1 jev, watch it improve until dominant" was first read
  as a tournament; the user clarified the intent is for **us** to learn which scenarios and what
  language/facts work. The arena therefore runs **2 jev + 1 shadow `jev_static` + 3 q3** — more
  decisions per minute, plus the model's would-be answers where q3 decided alone.
- The corpus drafted on 2026-11-06 was produced under Plan 72's calibration step; it moves under
  this plan's T1 because the lab is now its first consumer. Nobody has scored it against the model.

- **T2 (2026-11-06):** the variant set is the plan's four families made concrete — facts:
  `drop-dmg`, `drop-loc`, `drop-map`, `add-threat` (a derived `threat none|low|medium|high` line
  from enemies × distance × our band/weapon/armor); form: `hp-band-only`, `add-summary` (one of
  eight closed sentences); options: `options-consequence`; instructions:
  `instruction-criterion`, `instruction-role`. Each is a pure function over the rendered closed
  text and `&'static str` literals — a variant can never inject file or wire text. The
  thresholds the metrics use are the config's (`jev.gate_margin_*`), i.e. the provisional
  0.50 / 0.30 until Plan 72's W2 freezes them.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| Region layer (Plan 72 T13–T15) wording through the lab | regions are another question; same method applies | after Plan 72 T14 |
