# Plan 74 — Jev pickup objective — Tracker

## Overview

- Status: 0% complete (0/9 tasks)
- Start date: — (set when T1 starts, `date +%F`)
- Plan: `context/plans/74_jev_pickup_objective.md`
- Headline metric: per-life **`secs_to_strong`** (spawn → first non-weak held weapon) per arm;
  secondary: weak-weapon share of posture decisions (Plan 73 baseline **76 %**, 1 153 / 1 510).
- Arms: `q3` (no pad knowledge), `jev` (model picks the pad), `jvr` (code rule picks from the
  same candidates; model answer journalled as shadow).

## Resume Instructions

1. Read `AGENTS.md`, `../CLAUDE.md`, `context/plans/RULES.md` **in full**, this plan,
   `context/distilled/jev.md`, `context/distilled/jev_lab.md` (rules R4-2 and the arena
   conclusion), and the closed Plans 72/73 trackers in `context/plans/completed/`.
2. Order: T1 → T2 → T3 → T4 → T5 (W1) → T6 (W2) → T7 → T8 → T9. T2 does not need T1.
3. **Live work needs permission.** T6 calls `cosmo.lan:8710`; T8 runs the Q2 server. Agent-run
   only after the user says so in-session. Use scratch configs — never edit the user's
   `config.yaml`.
4. **Guard rails**: corpus committed and skimmed **before** scoring; holdout once; threshold only
   from `calibrate`; predictions committed before each arena round; W2 failure ⇒ stop the model
   half, never loosen the threshold.
5. Commit per task as `task(TN): …` with this tracker's row in the same commit, chained on
   `just all` (`just all > log && git add <paths> && git commit …`). Statuses: `pending` |
   `in-progress` | `done` | `blocked` | `skipped`.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Confirm hub keying, DM autoswitch, pad counts | tracker Evidence | pending | |
| 2 | T2: `MapItem.weapon` — weapon vs ammo pads | `brains/core.rs`, `items.rs`, `weapons.rs` | pending | |
| 3 | T3: Pad candidates + objective-only `pads` line | `brains/jev/policy.rs`, `jev/state.rs`, `qbots::jev::render` | pending | |
| 4 | T4: Objective question v2, roaming triggers, apply/release | `jev/question.rs`, `brains/jev/policy.rs`, `qbots::jev::hub` | pending | |
| 5 | T5: `jvr` code-rule control arm | `brains/jev`, `brains/mod.rs`, hub | pending | |
| 6 | T6: Corpus, lab, holdout, calibrate (live) | `context/fixtures/jev/calibration/objective_v2/**`, `lab.rs`, `jev_lab.md` | pending | |
| 7 | T7: Objective outcomes + per-life `secs_to_strong` | `jev/source.rs`, `brains/jev/policy.rs`, `main.rs`, `journal.rs` | pending | |
| 8 | T8: Arena rounds (live) | `context/fixtures/jev/arena_74.yaml`, reports | pending | |
| 9 | T9: Findings, docs, close | `jev_lab.md`, `BRAINS.md`, `brain_notes.md` | pending | |

## Gates

| Gate | After | Condition | Result |
|---|---|---|---|
| W1 | T5 | `just all` green; `q3_parity_golden` + posture render goldens unchanged | — |
| W2 | T6 | holdout threshold with CW = 0 and leakA ≤ 20 % — else stop the model half | — |

## Rounds (lab variants and arena rounds)

| Round | Change | Prediction (written first) | Result |
|---|---|---|---|

## Evidence

| What | Value | Source |
|---|---|---|
| Plan 73 weak-weapon share of posture decisions | 1 153 / 1 510 = 76 % (blaster 727) | plan *Pre-Identified Issue* command, 2026-11-07 |

## Notes / Deviations

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
