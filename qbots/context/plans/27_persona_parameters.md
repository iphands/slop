# Plan 27 — Persona parameters: a real per-bot personality for `main`

> **Status**: pending (file authored 2026-07-09; SERIES row existed since Plan 23 split)
> **Created**: 2026-07-09
> **Depends on**: Plan 25 (multibrain select + per-bot config)
> **Goal**: Expand `BotSkill`/`Personality` into a full per-bot persona (aggression, weapon preference, risk tolerance, reaction, camper/roamer, chattiness of movement) wired from fleet config and competition flags — so a `main` fleet reads as *different people*, the way the q3 roster (grunt/major/sarge/camper) already does.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: `BotSkill` (`crates/brain/src/skill.rs:59-77`) already has skill 0–10, Eraser
ratings, `Personality::{Conservative,Balanced,Aggressive}`, `quad_freak`, `camper` — but two
fields are **reserved and unused** (`preferred_weapon`, `reaction_time`), the personality
enum is coarse, and `main`'s tactical constants (flee/kite thresholds, strafe period, chase
budget) are global `const`s. Turn these into persona-driven parameters with named presets,
mirroring the q3 roster UX (`--q3char` → `--persona` for main).

**Deliverables**:
1. `Persona` struct (continuous [0,1] traits, q3char-style): `aggression`, `risk_tolerance`,
   `weapon_pref: Option<Weapon>`, `reaction_scale`, `camper`, `chase_commit`,
   `item_greed` — consumed by `main` (thresholds become functions of persona).
2. Named presets (e.g. `rusher`, `sniper`, `scavenger`, `guard`) selectable via
   `--persona`/`[fleet].persona`/`competition --personas`, with distinct names/skins.
3. The reserved `BotSkill` fields either wired or removed (no dead config).
4. Live roster proof: a 4-persona `main` fleet shows a distinct, explainable K/D +
   behavior spread (like the q3 roster table in `mode_perf.md`).

**Estimated effort**: Medium (1 day).

## Context

- Precedent: `Q3Character` (`q3char.rs:36-71`) — 14 [0,1] traits + presets, threaded via
  `build_brain(..., char)` and `--q3char`/`competition --q3chars` (Plan 38). Copy that
  plumbing shape exactly; don't invent a second config style.
- `main` constants that become persona-driven: `FLEE_HEALTH=30`, `KITE_HEALTH=50`,
  strafe-juke period (0.6s, Plan 45), weapon-rush thresholds, roam dwell (`camper` ×5),
  heatmap weights (`skill.rs:194` already personality-keyed — fold in), chase budget
  (Plan 29) and matchup strictness (Plan 28) once those land.
- Ordering note: 27 is numbered before 28/29 (they consume persona), but the *minimal*
  `Persona` + plumbing can land first and gain consumers as 28/29/30 ship. Implement 27
  first; keep the trait list additive.

## Step-by-Step Tasks

### T1: `Persona` type + presets (pure)

**File**: `crates/brain/src/skill.rs` (or new `persona.rs`)

**What to do**: Define `Persona` with the traits above + `Persona::from_skill(level,
Personality)` (defaults preserving today's behavior exactly) + 4 named presets. Unit tests:
default persona reproduces current constants; presets differ on the intended axes.

### T2: `main` consumes persona

**File**: `crates/brain/src/brains/main.rs`

**What to do**: Replace the global tactical `const`s with persona lookups (behavior-
preserving at default persona). `weapon_pref` biases `select_best_weapon` scores (~10%,
never overriding a dominant matchup). `camper`/`item_greed` bias roam-dwell and item goals.

### T3: Config/CLI plumbing

**Files**: `crates/brain/src/brains/mod.rs` (`build_brain`), `crates/qbots/src/main.rs`,
fleet config, competition

**What to do**: `--persona <name>` on connect-one/run/spawn-*/competition (`--personas`
matrix), `[fleet]` per-bot `persona` key — exactly parallel to `--q3char` (Plan 38 commits
are the template). Distinct default names/skins per preset for scoreboard readability.

### T4: Live roster proof + notes

**What to do**: `competition --brains main --personas rusher,sniper,scavenger,guard
--count 2` (8 bots, 5 min). Record the spread table in `context/mode_perf.md` (like the q3
roster table) + append `context/brain_notes.md`. The gate is *distinctness with intent*
(rusher trades most, sniper best K/D at range, scavenger tops item pickups), not balance.

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/skill.rs` / `persona.rs` | `Persona` + presets | P0 |
| `crates/brain/src/brains/main.rs` | consts → persona | P0 |
| `crates/brain/src/brains/mod.rs`, `crates/qbots/` | plumbing (`--persona`) | P0 |
| `context/mode_perf.md` | roster spread table | P1 |

## Open Questions / Risks

1. **Trait sprawl** — q3char has 14 and several are barely used. *Mitigation*: start with
   the 7 listed; add only when a consumer plan (28/29/30) needs one.
2. **Default-persona drift** (accidentally changing `main`'s tuned Plan 45 behavior).
   *Mitigation*: T1's reproduce-the-constants unit test is the contract.
3. **Two persona systems** (BotSkill ratings vs Persona). *Mitigation*: Persona wraps/owns
   `BotSkill`; remove the dead reserved fields in T1.

## Verification Checklist

- [ ] T1: default `Persona` reproduces current constants (unit-tested); presets defined; commit.
- [ ] T2: `main` reads persona everywhere the old consts lived (grep proves no orphans); commit.
- [ ] T3: `--persona` works on all four CLI paths + fleet config; commit.
- [ ] T4: 8-bot roster run recorded in `mode_perf.md` with an explainable spread;
      `brain_notes.md` appended; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
