# Xonotic roster, tuning & acceptance — Tracker

## Overview
- Status: 0% complete
- Start date: —
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
| 1 | T1: roster surfaces | main.rs, config.rs, supervisor.rs | pending | |
| 2 | T2: acceptance rows + baseline | tools/acceptance.rs, acceptance.md | pending | |
| 3 | T3: tuning loop | xonchar.rs presets, mode_perf.md | pending | N≥3 runs + control |
| 4 | T4: strategy token (conditional) | supervisor.rs, xon/goals.rs | pending | skip w/ evidence if timing fine |
| 5 | T5: docs + close series | brain_notes, README, SERIES | pending | git mv 62 to completed/ |

## Verification

- [ ] 4 presets distinct on the wire (names/skins/legend, ≤15 chars)
- [ ] Acceptance matrix xon rows pass; xg A/B recorded; EVT gates clean
- [ ] Tuning table in mode_perf.md (or documented indistinguishability)
- [ ] Plans 58–62 all in `completed/`, SERIES done
- [ ] Zero warnings, clippy clean, fmt, tests green at every commit (Rule A/B)
