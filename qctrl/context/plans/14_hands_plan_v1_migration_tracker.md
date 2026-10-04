# Plan 14 — hands-plan v1 Migration — Tracker

## Overview

- Status: 0% complete (0/7 tasks)
- Start date: — (set when T1 starts)
- Plan: `context/plans/14_hands_plan_v1_migration.md`
- Evidence: probe and scan output from `$SKILL_DIR/bin/hp-probe` / `hp-scan`
  (`SKILL_DIR=~/.claude/skills/hands-plan`), recorded in the Evidence table below; baseline
  input = the 2026-10-04 `/hands-plan resync` (0 CONFLICT · 0 TIGHTENING · 8 ADD · 12 UPDATE ·
  2 SHAPE · 7 INFO).

## Resume Instructions

1. Read the parent `../CLAUDE.md` (git discipline), the root `AGENTS.md`,
   `context/plans/RULES.md` **in full** (it changes under you — re-read it after T1, T2, T3),
   then `14_hands_plan_v1_migration.md`.
2. Environment:
   - The skill lives at `~/.claude/skills/hands-plan` (templates, `bin/hp-probe`,
     `bin/hp-scan`, `reference/sections.md`).
   - All edits are docs. The project gate still runs before every commit.
   - **Never** run `just fe-test` or `npm run testall` (they drive the live server).
3. Ordering:
   - T1 → T7 strictly in order. T1 lands the commit format every later task uses; T2's gate
     must land before T3's "This file wins"; T6 needs RULES authoritative; T7 is last.
   - T2, T3, T5 and T6 need the operator's input: gate wording and the release-build step,
     plan-gate rows, north star, and per-item root diffs.
4. Commit per task as `[qctrl][P14][T<n>][<topic>] <summary>`, with **this tracker's row in the
   same commit**. Stage explicit paths; chain edit `&&` commit; never push.
5. Started-state flip (T1's commit): plan `Status: in-progress`, SERIES row `in-progress`, the
   `Start date` here. SERIES has no *Currently Active* section until T5 adds it.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: per-task rules — commit format (B), append-only (B2), lifecycle (C), harvest (D), evidence (E), Project/Series rules | `context/plans/RULES.md` | pending | |
| 2 | T2: the verification gate — full pre-commit set, backend + frontend, baseline run | `context/plans/RULES.md` (Rule A) | pending | |
| 3 | T3: plan/tracker format, header, plan-gate, naming, style, templates + NN_example skeletons | `context/plans/RULES.md`, `NN_example*.md` (new) | pending | |
| 4 | T4: `context/AGENTS.md` + `context/CLAUDE.md` link + `distilled.md` | `context/` (new files) | pending | |
| 5 | T5: SERIES v1 structure | `context/plans/SERIES.md` | pending | |
| 6 | T6: root `AGENTS.md` block, de-dup, dead refs + root `CLAUDE.md` link (per-item approval) | `AGENTS.md`, `CLAUDE.md` (new) | pending | |
| 7 | T7: `hp:<id>` markers + hands-plan v1 stamps; clean resync | RULES, SERIES, `NN_example*`, `context/AGENTS.md` | pending | |

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped` | `invalid` —
the last three always carry a reason in the row's Notes cell.

## Evidence

| Date | What ran (command / build / input) | Result | Runs | Spread / notes |
|------|------------------------------------|--------|------|----------------|
| 2026-10-04 | `hp-scan --brief .` (pre-migration) | RULES/SERIES `stamp=none`; NN_example*, `context/AGENTS.md` missing; `ROOTFILE … block=none`; 4 DEADREF | 1 | baseline, quoted in the plan's Pre-Identified Issue |
| 2026-10-04 | `/hands-plan resync` | 0 CONFLICT · 0 TIGHTENING · 8 ADD · 12 UPDATE · 2 SHAPE · 7 INFO | 1 | the checklist this plan executes |

## Notes / Deviations

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
