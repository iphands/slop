# Plan 14 — hands-plan v1 Migration — Tracker

## Overview

- Status: 43% complete (3/7 tasks)
- Start date: 2026-10-04
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
| 1 | T1: per-task rules — commit format (B), append-only (B2), lifecycle (C), harvest (D), evidence (E), Project/Series rules | `context/plans/RULES.md` | done | Rule B rewritten to the template plus the `[qctrl][P<n>][T<n>][<topic>]` format; NOTE lines kept verbatim as its project addendum. Added B2, C (replaces the unlettered `## Completed Plans`), D, E, Project Rules, Series-Scoped Rules. 39/39 probes HIT |
| 2 | T2: the verification gate — full pre-commit set, backend + frontend, baseline run | `context/plans/RULES.md` (Rule A) | done | Heading renamed to "The verification gate" (operator, 2026-10-04); template preamble; project gate per component (backend 1–4, frontend 5 vitest-only, docs-only = steps 1–3 by operator choice), blind spots, build-loaded, look-at-it, prerequisites. Baseline 1–5 green. 17/17 probes HIT |
| 3 | T3: plan/tracker format, header, plan-gate, naming, style, templates + NN_example skeletons | `context/plans/RULES.md`, `NN_example*.md` (new) | done | Header ("This file wins"), plan-gate with the operator's 3 rows, naming, plan format (addendum: dependency matrix), tracker format, style (addendum: absolute paths), Templates & History; Mandatory Header and the embedded skeleton removed. Per-task rules byte-identical. Both skeletons created (stamp deferred to T7). 33 + 19 probes HIT |
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
| 2026-10-04 | T1 Verify: 4 `hp-probe` sets (commit, append-only, lifecycle, harvest+evidence+series+project) | 39/39 HIT; rules in order A, B, B2, C, D, E; no `{{`, no legacy `## Completed Plans` | 1 | |
| 2026-10-04 | T1 gate: `cargo fmt --all --check`, `cargo build`, clippy `-D warnings`, `cargo test --all-targets --all-features`, `npm run lint`, `npm run test` | fmt OK; 0 warnings; 163 passed / 0 failed (2 ignored); lint OK; vitest 31/31 | 1 | docs-only change; `testall` NOT run (live server) |
| 2026-10-04 07:52 | T2 baseline: gate steps 1–5 on the untouched tree (no `qctrl-api` running) | all pass: fmt OK; clippy 0 warnings; `cargo test` 163 passed / 0 failed (2 ignored); `RUSTFLAGS="-D warnings" cargo build --release` OK, 22.6 s; lint OK, vitest 31/31, `vite build` OK | 1 | recorded in the gate's Baseline line |
| 2026-10-04 | T2 Verify: `hp-probe` gate preamble (7) + gate-project shape (10) | 17/17 HIT; no `{{` | 1 | commit gate = docs-only steps 1–3, re-run after the edit |
| 2026-10-04 | T3 Verify: `hp-probe` RULES format/header/naming/style/templates (33), `NN_example.md` (12), `NN_example_tracker.md` (7) | all HIT; no foreign text or `{{`; dead refs `RULES.md:169` and `AGENTS.md:60` gone; RULES per-task section (old 127–311 vs new 99–283) `diff` IDENTICAL | 1 | gate (docs-only 1–3): fmt OK, clippy 0, `cargo test` 163 passed |

## Notes / Deviations

- **T1 — temporary dead reference, not predicted by the plan.** Rule D's template text routes
  findings via `context/AGENTS.md`, which T4 creates. So `hp-scan` now reports
  `DEADREF context/plans/RULES.md:226 → context/AGENTS.md`, the same kind as the
  `SERIES.md:42` one from the Plan 14 row. Both clear at T4; T4's Verify should show neither.
- **T1 — *Currently Active* doesn't exist yet** (plan Risk 5). The started-state flip set the
  plan `Status`, the SERIES row (`in-progress`) and the `Start date`; T5 adds the section.

## Follow-ups

| Follow-up | Why | Lands in |
|-----------|-----|----------|
