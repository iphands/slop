# Plan 73 — Jev lab + arena — Tracker

## Overview

- Status: 67% complete (6/9 tasks)
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
| 1 | T1: Lock the scenario corpus (human), split | `context/fixtures/jev/calibration/**` | done | 138 states (56 in `34f28aefa` + 82 here), all drafted by the agent, **skimmed and approved by the user 2026-11-06 before any scoring**; every half ≥10 obvious / ≥10 ambiguous, flat cases in both halves |
| 2 | T2: `jev-probe lab` — variants, split, scoring | `crates/qbots/src/jev/lab.rs`, `probe.rs` | done | 10 closed variants (baseline + 3 drop-a-fact, add-threat, hp-band-only, add-summary, consequence options, criterion / role instructions); FNV-1a split; holdout needs `--confirm-holdout`; `--split-report` is offline. 7 tests |
| 3 | T3: Run the lab, write findings | `context/distilled/jev_lab.md` | done | 4 rounds, 16 pre-registered predictions (9 right — 2 of them only partly — and 7 wrong; see Rounds), 1 099 asks, 0 failed; winner = coded + threat + graded summary, objective `seek_health / seek_armor / carry_on` |
| 4 | T4: Port winner, holdout once, calibrate | renderer, question specs, goldens | done | schema v2 (`5bbb4d754`); holdout accepted; posture threshold frozen **0.30**; objective **failed calibration** → parked, arena shadow judges it; Plan 72 W2 endpoint checks recorded |
| 5 | T5: Decision journal + outcome windows | `brain::jev::source` (`Outcome`, `outcome()`), `brains/jev/policy.rs`, `qbots::jev::hub` (journal), config `jev.journal` | done | window opens on resolution, closes at 10 s / death (hp ≤ 0 — a level change is not a death) / level change / supersede; records hp/armor/frags deltas, damage, q3's own retreat wish, whether the bias was in force; hub joins it with state + answer and a dedicated thread appends `logs/jev/journal-<unix>.jsonl`. 3 tests |
| 6 | T6: Shadow mode for `jev_static` | `brains/jev`, `brains/mod.rs`, `jev::hub`, config `jev.shadow`, supervisor | done | posture only (the objective is parked — asking it in shadow would need placeholder goals; follow-up); acks `Shadowed`, asks counted `shadow_asked`; parity test over 150 ticks |
| 7 | T7: Arena roster + `jev-probe journal` | `arena.yaml`, `journal.rs`, `probe.rs` | pending | offline |
| 8 | T8: Arena rounds (live, repeating) | Rounds table, `jev_lab.md` | pending | **live** — needs Q2 server + permission |
| 9 | T9: Hand evidence to Plan 72; close | Plan 72 tracker, pitfalls, brain_notes | pending | |

## Rounds (lab variants and arena rounds)

Every row is written **before** its run (prediction), then completed.

| # | Date | Loop | Change (one) | Prediction | Result | Conclusion |
|---|------|------|--------------|------------|--------|------------|
| L1 | 2026-11-06 | lab (tuning half) | baseline | posture right-and-confident (RC) **≤ 40 %** at t = 0.50 — the 2026-11-07 probes put an obviously bad state at margin 0.33; objective RC ≤ 50 % at t = 0.30; confident-wrong ≈ 0 | | posture RC 0 %, acc 82 %, mC 0.229; objective RC 75 %, leakA 60 %, flatA 0.637 | **right** — and starker: posture never clears 0.50 |
| L2 | 2026-11-06 | lab (tuning half) | drop-dmg | posture RC **down** a little (recent damage is evidence of danger); objective unchanged | | posture mC 0.229→0.253, RC 0 %; objective unchanged | **wrong** — the model is not reading `dmg` |
| L3 | 2026-11-06 | lab (tuning half) | drop-loc | **no change** on either question (exit counts barely bear on either decision) | | no RC change on either | **right** |
| L4 | 2026-11-06 | lab (tuning half) | drop-map | objective RC **down** (the `wants:` line names the gap the objective answers); posture unchanged | | objective acc 100→50 %, RC 75→8 %, CW 8 % | **right, far bigger than predicted** — `wants:` is load-bearing |
| L5 | 2026-11-06 | lab (tuning half) | add-threat | posture RC **up** clearly (an explicit danger level); objective small change | | posture acc 100 %, flatA 0.507→0, but mC 0.300 (RC 0 %) | **wrong** — raised accuracy, not confidence |
| L6 | 2026-11-06 | lab (tuning half) | hp-band-only | **no change** (the band was already present next to the number) | | no change | **right** |
| L7 | 2026-11-06 | lab (tuning half) | add-summary | **largest** posture gain of any variant (plain English, like the 0.880 "cannot win" probe); objective up | | posture RC 0→73 %, acc 100 %, mC 0.545; objective RC 100 %; posture leakA 29 % (all `lo` states) | **right** — largest gain; its leak comes from calling 25–49 hp "badly hurt" |
| L8 | 2026-11-06 | lab (tuning half) | options-consequence | posture RC **up** (options that say what happens are easier to weigh); objective up a little | | posture acc 82→64 %, mC 0.185 | **wrong** — consequence wording hurt |
| L9 | 2026-11-06 | lab (tuning half) | instruction-criterion | posture RC **up** and ambiguous leakage **up** too (a stated rule pushes borderline cases over) | | posture unchanged; objective RC 83 %, flatA 0.756 | **wrong** on posture — a stated rule did nothing |
| L10 | 2026-11-06 | lab (tuning half) | instruction-role | **no change** (role framing adds no facts) | | posture mC 0.229→0.133; objective RC 50 % | **wrong** — role framing hurt |
| L11 | 2026-11-06 | lab (tuning half) | items-renamed (`items health:none` → `pickups health_pack:not_seen` …) | both questions' **flatA drops** (objective 0.637 → < 0.40, posture 0.507 → < 0.30); obvious accuracy unchanged | | objective flatA 0.637→**0.724** (still seek_health); acc posture 82→64 %, objective 100→75 %; posture flatA 0.507→0.256 | **wrong** — renaming hurt, and finding 4's cause is retracted |
| L12 | 2026-11-06 | lab (tuning half) | summary-graded ("badly hurt" only < 25 hp, "hurt" 25–49) | posture **leakA 29 % → ≤ 10 %** at a small RC cost (73 % → ≥ 60 %); objective ≈ unchanged | | posture RC 73→**82 %**, leakA 29→**0 %**, flatA 0; objective RC 100 % | **right on leakage, wrong on cost** — RC rose instead of falling |
| L13 | 2026-11-06 | lab (tuning half) | summary-graded + items-renamed (combination of L11 + L12) | **best overall**: posture RC ≥ 60 %, leakA ≤ 10 %, flatA ≈ 0; objective RC ≥ 90 %, flatA < 0.40 | | posture RC 45 %; objective acc 83 % | **wrong** — the rename drags the combination below L12 alone |
| L14 | 2026-11-06 | lab (tuning half) | summary-graded + add-threat (combination) | posture RC ≥ L12 and flatA 0 (threat removed flat disengages in round 1); objective leakA up (threat raised it in round 1) | | posture RC 82 %, mC 0.563, sep **0.355**, leakA 0, flatA 0; objective leakA 70 % | **right** — best posture wording so far; objective leak up as predicted |
| L15 | 2026-11-06 | lab (tuning half) | objective only: summary-graded + a 4th option `carry_on` ("no particular objective, keep roaming"), the objective's harmless default (tests Plan 72's ≤3-options / no-none-of-the-above rule) | objective **flatA (acting) < 0.30** (blanks go to `carry_on`), **leakA ≤ 40 %** (from 60 %), RC ≥ 85 % (some `hold_position` mass moves to `carry_on`), CW 0; posture identical to summary-graded | | objective flatA 0.527→**0.271**, leakA 60→**40 %**, sep 0.233→**0.356**; but RC 100→**75 %**, CW **17 %** — all 3 obvious `hold_position` cases answered `carry_on` | **right** on blanks and leakage, **wrong** on RC: `hold_position` and `carry_on` are the same answer without a reason to hold a place |
| L16 | 2026-11-06 | lab (tuning half) | the **production candidate**: state = coded + `threat` + graded summary for both questions (one state per request); objective options `seek_health / seek_armor / carry_on` (variant `summary-graded-threat`) | posture as L14 (RC 82 %, leakA 0, flatA 0); objective **RC ≥ 90 %, CW ≤ 10 %, flatA ≤ 0.30, leakA ≤ 40 %** | | posture RC 82 %, CW 0, acc 100 %, leakA 0, flatA 0; objective RC **100 %**, CW 0, acc 100 %, leakA 40 %, flatA **0** — every blank state answers the no-op | **right** on every bound — adopted as the production wording |
| H1 | 2026-11-06 | **holdout, once** (production wording, `--confirm-holdout`) | none — confirmation of L16 | **accept** if holdout RC is within 10 points of tuning (posture ≥ 72 %, objective ≥ 90 %) and CW ≤ 5 % on both; otherwise record the overfit and fall back to baseline wording | | posture RC **78 %** (tuning 82), objective RC **100 %** (tuning 100), CW 0 on both, acc 100 % on both, flatA 0 | **ACCEPTED** — the wording generalises to unseen cases |
| C1 | 2026-11-06 | `calibrate` on the holdout (production wording, acting answers only) | none | a threshold exists for both questions; posture ≈ 0.40–0.50, objective ≈ 0.30–0.50 | | posture: admissible thresholds **[0.138, 0.47]** (obvious disengage margins 0.56–0.71, ambiguous disengage ≤ 0.14), tool's lowest = 0.138, **frozen 0.30** (the middle); objective: **no threshold** — 14/20 'ambiguous' holdout cases answered seek_armor/seek_health at 0.16–0.84 | **wrong on posture** (lower than predicted; tool's knife-edge minimum replaced by the interval's middle, stated openly); **objective fails** — not built (pre-registered rule) |
| A0 | 2026-11-06 | arena (smoke, **not pre-registered**) | first live run: posture live at 0.30 | — (no prediction written; recorded as a smoke) | jev K/D 0.40 vs q3 0.86 / jev_static 0.67; **disengage 64/69**; jev env deaths 5 (4 drown) | live state mix ≠ corpus mix (weak-weapon contact dominates); q3 retreat is hazard-blind — see `jev_lab.md` A0 |

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-11-06 | `cargo run -p qbots -- jev-probe lab --split-report` (offline, corpus @ `34f28aefa`) | posture: tuning 6 obv / 5 amb / 3 flat, holdout 8 / 6 / 0; objective: tuning 9 / 5 / 3, holdout 5 / 6 / 0 | 1 | no model call; confirms T1 must grow the corpus |
| 2026-11-06 | `jev-probe health` | PASS — slug matches, chat route answers 501 | 1 | endpoint permission granted by the user in-session 2026-11-06 |
| 2026-11-06 | `jev-probe --json lab --half holdout --confirm-holdout --variant summary-graded-threat --detail …` (**the one holdout use**; 67 asks, 0 failed) | posture RC 78 %, objective RC 100 %, CW 0 — accepted | 1 | `holdout_production{,.detail}.jsonl` |
| 2026-11-06 | `jev-probe calibrate` (production wording, holdout only, acting answers) | posture 0.138 (lowest admissible; range to 0.47) → frozen 0.30; objective: no threshold | 1 | same answers as the holdout run (deterministic) |
| 2026-11-06 | `jev-probe sensitivity` / `anticorrelation` / `flatness` | posture pair moves (L1 0.99) ✓; objective pair does **not** (L1 0.013 — both states are a 20 hp bot, health need swamps armor distance; the pair is badly chosen) ✗; anticorrelation ✓; flatness ✓ (3-way top 0.36, 24-way top 0.053, both rejected) | 1 | |
| 2026-11-06 | `jev-probe bench --secs 30` (8 bots) and `--bots 36` | 0.53 req/s and 2.40 req/s: p95 **16 ms** (deadline 350), 458 input tok/s at 36 bots = 12 % of single-stream | 2 | loopback; requests are posture-only |
| 2026-11-06 | `jev-probe --json lab --half tuning --variant summary-graded-threat --detail …` (round 4, production candidate; 71 asks, 0 failed) | posture RC 82 %, objective RC 100 %, CW 0, every blank → no-op | 1 | `round4_tuning{,.detail}.jsonl` |
| 2026-11-06 | `jev-probe --json lab --half tuning --question objective --variant summary-graded-carry-on --detail …` (round 3; 34 asks, 0 failed) | `context/fixtures/jev/lab/round3_tuning{,.detail}.jsonl`; findings R3-1…R3-2 | 1 | |
| 2026-11-06 | `jev-probe --json lab --half tuning --variant <v> --detail …` × 4 (round 2: items-renamed, summary-graded, summary-graded-items-renamed, summary-graded-threat; 284 asks, 0 failed) | `context/fixtures/jev/lab/round2_tuning{,.detail}.jsonl`; findings R2-1…R2-4 in `jev_lab.md` | 1 | |
| 2026-11-06 | `jev-probe --json lab --half tuning --detail …` (round 1, 10 variants × 71 tuning states = 710 asks, 0 failed, ~10 s) | results + per-case detail in `context/fixtures/jev/lab/round1_tuning{,.detail}.jsonl`; summary in `context/distilled/jev_lab.md` | 1 | deterministic model — a re-run reproduces it |
| 2026-11-06 | `jev-probe lab --split-report` on the locked corpus (138 states) | posture: tuning 11 obv / 14 amb / 12 flat, holdout 18 / 10 / 2; objective: tuning 12 / 10 / 12, holdout 15 / 20 / 2 | 1 | offline; T1's split requirement met |
| 2026-11-07 | Plan 72 review probes (carried over as motivation) | same bad situation: disengage 0.441 (3-way) vs 0.663 (binary); explicit "cannot win" → 0.880 | 3 | framing moves the answer more than anything else we control |

## Notes / Deviations

- Origin (2026-11-06): the user's "4 q3 vs 1 jev, watch it improve until dominant" was first read
  as a tournament; the user clarified the intent is for **us** to learn which scenarios and what
  language/facts work. The arena therefore runs **2 jev + 1 shadow `jev_static` + 3 q3** — more
  decisions per minute, plus the model's would-be answers where q3 decided alone.
- The corpus drafted on 2026-11-06 was produced under Plan 72's calibration step; it moves under
  this plan's T1 because the lab is now its first consumer. Nobody has scored it against the model.

- **T4 (2026-11-06):** (1) the production renderer appends the round-4 `threat` and graded
  `summary` lines (schema v2) and a parity test pins it byte-for-byte to the lab variant; the
  questions live once, in `brain::jev::question`. (2) Holdout used **once**: accepted (posture
  78 %, objective 100 %, CW 0). (3) **Posture threshold frozen at 0.30**, not the tool's 0.138:
  every value in [0.138, 0.47] satisfies the pre-registered criteria, and the lowest sits one
  case above an ambiguous `disengage` (0.14); the middle is robust both ways. The calibrate rule
  ("lowest admissible") is kept as the reported number. (4) **The objective question fails
  calibration** — 14/20 holdout `ambiguous` cases draw confident `seek_armor`/`seek_health`, so no
  threshold keeps ambiguous leakage ≤ 10 % with ≥ 80 % obvious RC — and its sensitivity pair did
  not move. By the pre-registered rule it is **not built** into Plan 72 yet. It is not deleted
  from the lab: many of those answers look defensible (low armor with armor near), so the arena
  will ask it in **shadow** (never applied) and let outcomes judge both the question and the
  labels (plan risk 2). The sensitivity pair for it must be redesigned (a bot that *wants* armor).
- **Process failure, 2026-11-06 (T5):** commit `42bf607b5` says "Gate green" but `just all`
  had **failed** (clippy `large_enum_variant` on `JevStartup::Ready`, which grew when
  `HubConfig` gained the journal path). The bookkeeping script and the commit ran after the gate
  without being chained on it with `&&` — exactly the incident the project CLAUDE.md warns
  about. Fixed in the next commit (box the config); not amended. From here every task commit is
  chained on a passing gate.
- **T3 decisions (2026-11-06, delegated by the user — "you decide, best guess"):**
  (1) **adopt `carry_on`**: the objective options become `seek_health / seek_armor / carry_on`
  (still ≤ 3, so Plan 72's cap holds) and the 9 obvious `hold_position` cases are relabelled
  `carry_on` by `git mv` — *after* their tuning-half answers were seen, so those tuning cases are
  no longer blind; the holdout's were never scored and stay clean. Area control returns as its
  own question once regions exist (Plan 72 T14). (2) **keep the objective `ambiguous` labels the
  model answered confidently**: relabelling a case because the model disagreed is the bias the lab
  exists to prevent; the arena's outcomes judge them instead. Also: the raw harvest is gitignored.
- **T3 round 1 (2026-11-06):** the lab gained `--detail` (one line per case) and the
  `ambiguous_leak_acting` / `flat_max_acting` metrics mid-round: for posture, `hold` is the no-op
  ("leave it to q3"), so a confident `hold` on an ambiguous or blank state costs nothing — only an
  acting answer there changes play. The round was re-run with them (identical answers; the model
  is deterministic). `context/fixtures/jev/harvest.jsonl` (raw request/response pairs, ~0.6 MB per
  full round) is **not committed**: the corpus + code reproduce every answer, and the per-case
  detail files carry the probabilities.
- **T1 (2026-11-06):** batch 2 (82 states) took three generator runs because the FNV-1a split is
  decided by file name, not chosen: the first top-up left posture-tuning at 9 obvious,
  objective-tuning at 9 ambiguous and every flat case in tuning, so two small self-contained
  top-ups followed until each half met ≥10/≥10 with flat cases in both. One generator run also
  re-ran batch 1 by importing its script; the 56 byte-identical duplicates it wrote were detected
  against their committed twins and deleted before anything was committed — the committed batch
  1 was never modified. Flat states never contain an `enemy` line: "no enemy visible" is not
  information-free for posture (`hold` is the right answer there).
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
