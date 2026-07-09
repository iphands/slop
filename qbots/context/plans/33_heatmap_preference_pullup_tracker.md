# Plan 33 — Heatmap preference pull-up — Tracker

## Overview
- Status: 0% complete
- Start date: —
- Goal: `heatmap_weights` becomes a live persona function of mood (health/engagement);
  neutral mood byte-preserves today's weights; q3 unchanged.

## Resume Instructions
1. Read `33_heatmap_preference_pullup.md`. Current mapping: `skill.rs:194` (static per
   personality); consumed via `Brain::heatmap_weights()` (`brains/main.rs:144`).
2. Depends on Plan 27's `Persona` (`risk_tolerance`).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: mood-aware weights + trait signature | `persona.rs`, `brains/core.rs` + impls | pending | neutral == today (test) |
| 2 | T2: detour-by-mood test + live sanity | `brain/tests/`, `brain_notes.md` | pending | extend Plan 08 test |
