# Xonotic roster, tuning & acceptance — Tracker

## Overview
- Status: 100% complete (closed 2026-07-11)
- Start date: 2026-07-11
- Deliverable: xon personality roster on all selection surfaces; xon/xg in the Plan 47 acceptance matrix with recorded baseline; evidence-based tuning pass. Closes the Xonotic series (58–62).
- Blocked by: Plans 60 and 61 must be in `completed/` first.

## Resume Instructions
1. Read `context/plans/RULES.md`, Plan 62, and `completed/38_q3_personality_roster*` (the precedent).
2. Tuning rule: NO conclusions from single runs — always the Plan 47 aggregator with `--control`; record mean [min..max].
3. Live tasks need a q2 server; mark `blocked` if unavailable.
4. T4 (strategy token) is conditional — check Plan 60's tracker for the rating-session timing evidence first.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: roster surfaces | main.rs, config.rs, supervisor.rs | done | `26d39c317`. GroupChar axis; legend; 15-char test extended. Live 4-group roster verified. |
| 2 | T2: acceptance rows + baseline | tools/acceptance.rs, acceptance.md | done | `dc653e124`. --navmode passthrough; q2dm1 batches PASS (xon 2/3, xg 2/3). q2dm3/q2dm2 formal batches = operator-assisted follow-up (manual evidence in mode_perf). |
| 3 | T3: tuning loop | xonchar.rs presets, mode_perf.md | done | `f1ef4a98c`. N=3 + post-tune verify; shp in q3's band; rusher aim −1→+1, turtle 4→5. |
| 4 | T4: strategy token (conditional) | supervisor.rs, xon/goals.rs | skipped | Evidence: zero timing signal in any soak ≤10 bots; one flood/bot/7s + ordinal stagger suffices. |
| 5 | T5: docs + close series | brain_notes, README, BRAINS.md, SERIES | done | README/BRAINS.md list xon/xg + rosters; brain_notes series wrap; plan → completed/. |

## Verification

- [x] 4 presets distinct on the wire (names/skins/legend; `xon_xg_rus_999` = 14 chars)
- [x] Acceptance matrix q2dm1 rows pass (xon 2/3, xg 2/3); EVT gates clean in every soak (0 drown/panic)
- [x] Tuning table in mode_perf.md — presets distinguishable; 2 evidence-committed adjustments
- [x] Plans 59–62 in `completed/` (58 in `abandoned/` per user call), SERIES rows final
- [x] Zero warnings, clippy clean, fmt, tests green at every commit (Rule A/B)
