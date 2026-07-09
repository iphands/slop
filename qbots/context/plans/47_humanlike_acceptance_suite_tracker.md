# Plan 47 — Human-like play acceptance suite — Tracker

## Overview
- Status: 0% complete
- Start date: —
- Goal: one command runs the traversal matrix + behavior-counter competitions per brain;
  baseline recorded in `context/acceptance.md` as the series regression gate.

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
| 1 | T1: EVT behavior counters + aggregation | `brain/*`, `qbots/*` | pending | greppable one-liners |
| 2 | T2: acceptance driver | `justfile` / `tools/acceptance.rs` | pending | per-map batches |
| 3 | T3: baseline recorded | `context/acceptance.md` | pending | date + commit hash |
| 4 | T4: showcase run + narrative | `acceptance.md`, `brain_notes.md` | pending | q2dm3 roster match |
