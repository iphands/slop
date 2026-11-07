# Plan 74 — Jev pickup objective — Tracker

## Overview

- Status: 44% complete (4/9 tasks)
- Start date: 2026-11-07
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
| 1 | T1: Confirm hub keying, DM autoswitch, pad counts | tracker Evidence | done | all three resolved (Evidence). Hub: one pending + **one pacing clock** per bot → T4 gains per-question pacing and an asked-objective-only release rule. DM switches only from the blaster. `dmflags 17424`, no weapons-stay. q2dm1: 11 weapon vs 35 ammo pads |
| 2 | T2: `MapItem.weapon` — weapon vs ammo pads | `brains/core.rs`, `items.rs`, `weapons.rs` | done | `Weapon::from_classname` (yquake2 `g_items.c` classnames; ammo incl. `ammo_grenades` → `None`); 3 test constructors set `weapon: None`; new pak-gated `tests/map_items_q2dm1.rs` asserts 11 weapon / 35 ammo; parity golden unchanged; pitfalls entry |
| 3 | T3: Pad candidates + objective-only `pads` line | `brains/jev/pads.rs` (new), `brains/jev/policy.rs`, `jev/state.rs`, `qbots::jev::render` | done | `pads::candidates` = one `flood_costs` per ask; relevance = stronger weapon / hp < 100 / armor < 100; `ItemMemory` observed per tick, reset on map change. `FieldKey::Pads` (keep-priority 5), schema **v3**; `pads` added only via `DecisionState::with` in objective asks → posture golden byte-identical; new objective golden. 6 tests (see Deviations) |
| 4 | T4: Objective question v2, roaming triggers, apply/release | `jev/question.rs`, `jev/source.rs`, `brains/jev/{policy,pads,mod}.rs`, `qbots::jev::{hub,render}` | done | `objective_question_v2(Offered)` + `SEEK_WEAPON`; asks with no enemy in view, ≥ 2 s alive, on a pending event or objective refresh, never while held; resolves via the ticket's own pad list; held until arrival / pad empty / 30 s / stall / death / map change / newer **objective** ticket; `Trigger::Released`; hub pacing per question key + `refresh_due(key)`. 7 brain tests, 1 hub test, 1 render test |
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
| T1 hub keying | one `pending: Mutex<Option<Pending>>`, one `pacing`, one `decision: (AppliedDecision{objective, posture})` per bot; `submit` paces every ask on one clock (`event_floor` for events, `min_dwell` for refresh) | `crates/qbots/src/jev/hub.rs:176-197`, `:433-470`, `:613-650` |
| T1 DM auto-switch | `newweapon` set only if `inventory[index] == 1` and (`!deathmatch` or held == blaster) | `vendor/yquake2/src/game/player/weapon.c:325-330` |
| T1 server dmflags | `noir.lan:27910` status: `mapname q2dm1, dmflags 17424, maxclients 64, timelimit 900, fraglimit 0`, 0 players → 16 INSTANT_ITEMS + 1024 FORCE_RESPAWN + 16384 QUAD_DROP; **no** WEAPONS_STAY (4) | connectionless `status`, 2026-11-07; `shared.h:1152-1166` |
| T1 pads q2dm1 | weapon 11 (CG 1, GL 1, HB 1, MG 2, RG 1, RL 2, SG 2, SSG 1), health 18, armor 18, ammo 35 | BSP entity lump, `vendor/baseq2/pak0.pak` |
| T1 pads q2dm3 | weapon 8 (GL 1, HB 1, MG 1, RG 2, RL 1, SG 1, SSG 1), health 15, armor 8, ammo 19 | same |

## Notes / Deviations

- **T3:** the plan's "skip visible pads within 128 u" rule was **not** built — q3's own nearby-item
  grab (`SeekNbg`, ≤ 150 u) still runs while an objective is applied (the objective replaces only
  the roam fallback), so a near visible pad needs no special case. "Objective-only" is by
  construction rather than a renderer branch: `DecisionState::build` never adds `Pads`; the
  objective ask adds it with `with(Field::Pads(..))`.
- **T4:** additions beyond the plan text — `Trigger::Released` (slug `released`) for the re-ask
  after a release; a 1 s retry throttle per objective attempt (each attempt floods the graph); the
  2 s settle applies after a map change too. `DecisionSource::refresh_due` now takes the
  `QuestionKey` (trait change: hub, `NullDecider`, `StubDecider`).
- **T4:** a plain `jev_static` (`NullDecider`) now also *asks* the objective — a no-op (the null
  decider never answers), about one graph flood per life. The posture-only shadow (Plan 73's
  `jvs` with a decider) does **not** ask it; T5 gives `jvr` its own picker.
- **T4:** two test "failures" on the way were the release rules working: a bot standing still for
  5 s trips q3's stall detector (`on_stall` → release), and a pad within `ItemMemory`'s 500 u
  trust range that the view does not transmit reads as taken. The tests now walk the bot and show
  the stocked pad, as PVS would.
- **T3:** pad cost is the graph's edge-cost sum (world-unit scale, plus whatever penalties the
  graph carries for swim/jump edges), banded with the same `DistBand` edges as straight-line
  distances — "travel cost band", not a pure distance.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
| The visible `items weapon:` fact still counts ammo boxes (`EntityClass::ItemWeapon`) | it is a posture input; changing it would void posture's 0.30 calibration | a later plan (re-calibrate posture) |
