# Plan 27 — Persona parameters — Tracker

## Overview
- Status: 0% complete
- Start date: —
- Goal: `Persona` traits + presets for `main`, wired via `--persona`/fleet config
  (mirror Plan 38's `--q3char` plumbing); default persona byte-preserves current behavior.

## Resume Instructions
1. Read `27_persona_parameters.md`. Template plumbing: Plan 38 commits (`--q3char`).
2. T1's contract: default `Persona` reproduces today's constants (unit test first).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `Persona` type + 4 presets (pure) | `skill.rs`/`persona.rs` | pending | default == today (test) |
| 2 | T2: `main` consumes persona | `brains/main.rs` | pending | consts → lookups |
| 3 | T3: CLI/config plumbing | `brains/mod.rs`, `qbots/` | pending | `--persona`, `--personas` |
| 4 | T4: live roster proof + notes | `mode_perf.md`, `brain_notes.md` | pending | 8-bot spread table |
