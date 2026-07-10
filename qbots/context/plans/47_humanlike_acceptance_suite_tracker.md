# Plan 47 — Human-like play acceptance suite — Tracker

## Overview
- Status: in-progress (started 2026-07-10, **reordered earlier** at user request)
- Start date: 2026-07-10
- Goal: one command runs the traversal matrix + behavior-counter competitions per brain;
  baseline recorded in `context/acceptance.md` as the series regression gate.

## Reorder rationale (2026-07-10)
Plan 30's live A/B was **inconclusive due to variance** — a q3 CONTROL group's K/D swung
1.00→0.86→2.60 across three runs of *identical code*. Single-competition combat A/B is noise
(see `pitfalls.md`). So the behavior plans 28/29/33 CANNOT be verified until a **multi-run,
control-group measurement harness** exists. That harness is Plan 47's core, so 47 is pulled
ahead of 28/29/33. **First deliverable: the multi-run competition aggregator** (parse N
scoreboards → mean±spread per group + a signal-vs-noise verdict using the control's own spread).
The traversal matrix + showcase (original T2–T4) follow.

## Resume Instructions
1. Read `47_humanlike_acceptance_suite.md`. This is the capstone — sequence LAST (after
   35/42/43/46 and as many of 27–33 as have landed; rows for unlanded plans go in as
   "expected-fail" with a note, don't block on them).
2. No tmp scripts: the driver is a `justfile` recipe or `crates/tools/src/bin/acceptance.rs`.
3. Thresholds start at proven floors (`mode_perf.md`): q2dm1 swim reached, q2dm3 railgun
   ≥ 3/4, q2dm3 quad ≥ 3/4 (needs Plan 35), q2dm2 s2s 8/8.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 2a | **Multi-run aggregator (the reordered core)** | `tools/src/bin/acceptance.rs` | **done** | parse N scoreboards → mean±spread + signal-vs-noise verdict (control spread = noise floor); 4 unit tests; demonstrated on real q2dm1 data (main 0.49 vs q3 1.49 → inconclusive, q3 spread 1.74) |
| 1 | T1: EVT behavior counters + aggregation | `brain/*`, `qbots/*` | pending | greppable one-liners |
| 2 | T2: acceptance driver (`acceptance matrix`) | `tools/acceptance.rs` | in-progress | 4-row traversal matrix (q2dm1 swim, q2dm3 ride+quad, q2dm2 s2s) × `--brains`; per-map batches w/ operator prompts (`--yes` to skip — wrong map fails fast on scenario preflight); auto cache-regen per lift-penalty variant; pass/fail table + exit code; thresholds = proven floors w/ notes. 2 more unit tests |
| 3 | T3: baseline recorded | `context/acceptance.md` | partial | harness + variance demo recorded; matrix baseline pending |
| 4 | T4: showcase run + narrative | `acceptance.md`, `brain_notes.md` | pending | q2dm3 roster match |
