# Plan 33 — Heatmap preference pull-up: persona-owned danger/crowd weighting

> **Status**: pending (file authored 2026-07-09; SERIES row existed since Plan 23 split)
> **Created**: 2026-07-09
> **Depends on**: Plan 08 (heatmap), Plan 27 (persona)
> **Goal**: The danger/popularity *preference* becomes a brain/persona concern with richer inputs (health-state, engagement-state), while the nav layer keeps only the mechanical overlay — a hurt bot avoids hot lanes harder; a healthy rusher seeks them.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: The Plan 08 heatmap (`brain/src/heatmap.rs`: per-bot danger/popularity overlay,
A* edge cost `base + W_d·danger − W_p·popularity`) is already brain-owned and weighted via
`Brain::heatmap_weights()` → `skill.heatmap_weights()` (`main.rs:144`, `skill.rs:194`).
What's missing is *dynamism*: the weights are static per personality for the bot's whole
life. Make them a live persona function of the bot's state so routing mood changes like a
human's.

**Deliverables**:
1. `heatmap_weights()` becomes state-aware: scaled by health (hurt → danger weight up,
   popularity down), engagement (hunting a kill → tolerate danger), and Plan 27 persona
   traits (`risk_tolerance` replaces the coarse Personality mapping).
2. q3 keeps its current behavior (control brain) unless trivially additive.
3. A deterministic unit test (extend Plan 08's detour test) showing the same bot detours
   when hurt and cuts through when healthy-and-hunting.

**Estimated effort**: Small (2 h).

## Context

- `heatmap.rs` constants: `TAU_DANGER`, death bump, per-node cap — untouched here.
- `skill.rs:194 heatmap_weights()`: danger 30→150 by skill, popularity 8/20/40 by
  Personality. This mapping moves into `Persona` (Plan 27) and gains multipliers from live
  state passed through `BrainContext` (health is already in `SelfState`).
- Interface note: `Brain::heatmap_weights(&self)` takes no state today
  (`brains/core.rs`); either widen the signature or have `main` cache the last-tick state —
  prefer the signature change (all four brains touched, trivial).

## Step-by-Step Tasks

### T1: State-aware weights in persona

**Files**: `crates/brain/src/skill.rs`/`persona.rs`, `crates/brain/src/brains/core.rs`
(trait signature), all brain impls

**What to do**: `heatmap_weights(state: HeatmapMood)` where
`HeatmapMood { health_frac, engaged, hunting }`; persona maps mood → `(W_d, W_p)`
(defaults reproduce today's values at full health / idle — unit-tested, same discipline as
Plan 27 T1). `main` fills the mood from its FSM; `q3`/`sentry`/`runtester` pass a neutral
mood (behavior-preserving).

### T2: Deterministic detour test + live sanity

**Files**: `crates/brain/tests/` (extend the Plan 08 integration test)

**What to do**: Same graph, same danger field: hurt bot's path detours; healthy hunting
bot's path goes direct. Live sanity: 5-min fleet run, grep per-bot weight logs for the
mood swings. Append `context/brain_notes.md` (dated).

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/skill.rs`/`persona.rs` | mood-aware weight fn | P0 |
| `crates/brain/src/brains/core.rs` + impls | signature + mood plumbing | P0 |
| `crates/brain/tests/` | detour-by-mood test | P1 |

## Open Questions / Risks

1. **Weight thrash** (mood flips per tick → path flaps). *Mitigation*: smooth the mood
   (~1s EMA) before mapping; the Plan 08 replan cadence already dampens.
2. **Hidden q3 change** via the shared trait. *Mitigation*: neutral mood passthrough is
   byte-identical; assert in a test.

## Verification Checklist

- [ ] T1: neutral mood reproduces today's weights (unit test); mood mapping implemented; commit.
- [ ] T2: detour-by-mood deterministic test green; live log sanity; `brain_notes.md`
      appended; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
