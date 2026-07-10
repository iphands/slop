# Plan 27 — Persona parameters — Tracker

## Overview
- Status: **DONE (2026-07-10)** — persona mechanism + main consumption + connect-one selection
  shipped, all byte-preserving. Competition roster (`--personas`) + live roster proof are a
  documented mechanical follow-on (noise-limited per the harness lesson; no downstream plan needs
  persona SELECTION — 29/33 consume the traits with sensible defaults). Moved to `completed/`.
- Start date: 2026-07-10
- Goal: `Persona` traits + presets for `main`, wired via `--persona`/fleet config
  (mirror Plan 38's `--q3char` plumbing); default persona byte-preserves current behavior.

## Resume Instructions
1. Read `27_persona_parameters.md`. Template plumbing: Plan 38 commits (`--q3char`).
2. T1's contract: default `Persona` reproduces today's constants (unit test first).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: `Persona` type + 4 presets (pure) | `persona.rs` | done | default reproduces 30/50/450/50 exactly (unit-tested); rusher/sniper/scavenger/guard |
| 2 | T2: `main` consumes persona | `brains/main.rs` | done | FLEE_HEALTH/KITE_HEALTH/KITE_DIST/dwell consts → `self.persona.*()`; byte-preserving; no orphan refs |
| 3 | T3: CLI/config plumbing | `brains/mod.rs`, `qbots/` | done (partial) | `build_brain` persona param + `MainBrain::with_persona`; `connect-one --persona` (preset lookup, hard error on bad name). `--personas`/run/fleet-config = follow-on |
| 4 | T4: live roster proof | `mode_perf.md` | deferred | needs `competition --personas` wiring; kd spread noise-limited (harness lesson) — a roster demo, not a gate |
