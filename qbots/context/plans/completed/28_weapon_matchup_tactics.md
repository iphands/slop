# Plan 28 — Tactical weapon matchups: enemy-weapon inference + range-aware positioning

> **Status**: pending (file authored 2026-07-09; SERIES row existed since Plan 23 split)
> **Created**: 2026-07-09
> **Depends on**: Plan 27 (persona parameters), Plan 24 (main brain)
> **Goal**: Bots read the matchup like humans — infer the enemy's weapon from the wire, hold *their own* weapon's ideal range (rush with SSG, keep distance with rail/RL), and back off from fights their loadout loses.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Distance-aware weapon *switching* already exists and is mature
(`weapons.rs:206 score_weapon` + `select_best_weapon`, emitted as `use <name>` stringcmds by
`combat.rs:143-156` — close-range shotgun bonus, splash `min_safe_distance`, range falloff).
What's missing is the other half of "switching weapons for close/far combat": **moving to
the range where your weapon wins**, and reading **what the enemy holds**. `main` today
fights every enemy at fixed `IDEAL_DIST`/`BACKUP_DIST` constants regardless of loadouts.

**Deliverables**:
1. Enemy weapon inference from the wire (player entity `modelindex2` VWep model →
   `Weapon`), exposed on perceived players.
2. Per-weapon ideal-range model replacing `main`'s fixed `IDEAL_DIST`/`BACKUP_DIST`: SSG/CG
   push in, rail/RL hold out, blaster closes fast.
3. Matchup-aware engage bias: outgunned-at-this-range → reposition or disengage (feeds the
   Plan 29 chase/disengage decisions; reuses `q3char::bot_aggression` where applicable).
4. Unit tests for inference + range tables; live A/B vs the Plan 45 baseline.

**Estimated effort**: Medium (1 day).

## Context

### What exists (surveyed 2026-07-09)
- Switching: `crates/brain/src/weapons.rs` — full weapon table with `effective_range()`,
  `min_safe_distance()`, `power()`, `is_hitscan()`; `score_weapon` (:206),
  `select_best_weapon` (:236) with a 95% anti-thrash threshold; `combat.rs` emits
  `WeaponRequest` (`use <name>`; Q2 ignores `usercmd.impulse` — `g_cmds.c:1945`).
- Positioning: `brains/main.rs` uses fixed constants (`IDEAL_DIST`≈160u, `BACKUP_DIST`≈80u,
  `KITE_HEALTH=50`) — one distance policy for all loadouts.
- Own weapon known via `gunindex`→CS_MODELS (`perception.rs:132`,
  `Weapon::from_view_model`). **Enemy weapon not read** — `perception.rs:178` has the TODO.
- Plan 07 deferred "retreat-from-RL" as "enemy weapon not on wire" — that was wrong in the
  useful case: with VWep (stock in Q2 3.20+), player entities carry `modelindex2` = the
  wield-model (`#w_railgun.md2` etc.) resolvable through CS_MODELS, same trick as our own
  `gunindex`. Fallback when VWep is off: infer from observed projectiles/muzzle sounds, or
  assume unknown.
- Plan 45's finding motivates this: `main`'s residual K/D gap is *per-engagement combat
  quality*; range control is the next lever that isn't raw aim.

## Step-by-Step Tasks

### T1: Enemy weapon inference

**Files**: `crates/brain/src/perception.rs`, `crates/brain/src/weapons.rs`

**What to do**: For perceived player entities, resolve `modelindex2` through CS_MODELS to a
weapon wield-model name; add `Weapon::from_wield_model(path)` (parse `#w_*.md2` /
`models/weapons/w_*` forms — verify actual strings against `vendor/yquake2` VWep code and a
live capture). Store `Option<Weapon>` + `inferred_at` frame on the perceived player. Unit
tests with real configstring fixtures. Unknown stays `None` — never guess.

### T2: Per-weapon ideal-range model

**Files**: `crates/brain/src/weapons.rs` (pure fns), `crates/brain/src/brains/main.rs`

**What to do**: Add `ideal_range(weapon) -> RangeBand { close_in, hold, back_off }`-style
data (SSG ~<128u; RL 300–600u — inside `min_safe_distance` is suicide; rail: the longer the
better; CG/MG mid). In `main`'s engage movement, replace the fixed `IDEAL_DIST`/
`BACKUP_DIST` with the held weapon's band: advance when too far for our weapon, back up when
inside splash-danger or when the enemy's weapon beats ours at this range. Keep the Plan 45
strafe juke orthogonal (it composes with approach/retreat).

### T3: Matchup engage bias

**Files**: `crates/brain/src/brains/main.rs` (+ a pure helper in `weapons.rs`)

**What to do**: `matchup_score(mine, theirs, dist) -> f32` (pure, unit-tested): who wins at
this range if both aim well. Feed it into `main`'s existing kite/flee gates (from Plan 45)
as an additional trigger: e.g. blaster-vs-rail at 600u → close or break LOS (never stand and
trade); SSG-vs-SSG at 400u → rush. Persona (Plan 27) scales how strictly the bot obeys the
matchup (aggressive personas fight uphill). `q3` untouched (its aggression model already
covers this coarsely).

### T4: Live A/B + notes

**What to do**: 2× 5-min `competition --count 4 --brains q3,main --chars major
--navmodes hybrid-fallback` (the Plan 45 harness). Target: main kd > 0.68 baseline.
Also verify visually sensible logs: no standing rail-duels on blaster, SSG bots closing.
Append `context/brain_notes.md` (dated); pitfalls to `context/pitfalls.md` (especially the
real VWep string forms found in T1).

> **Rule B reminder**: commit after *each* task; fmt + clippy(-D warnings) + tests green.

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/perception.rs` | enemy weapon via `modelindex2`/CS_MODELS | P0 |
| `crates/brain/src/weapons.rs` | `from_wield_model`, range bands, `matchup_score` | P0 |
| `crates/brain/src/brains/main.rs` | range-band positioning + matchup gates | P0 |
| `context/brain_notes.md` | results | P1 |

## Open Questions / Risks

1. **VWep availability** — if the server runs without VWep, `modelindex2` won't carry the
   weapon. *Mitigation*: T1 verifies against our live server first (`connect-one` capture);
   inference stays `Option` and all consumers must handle `None` (fall back to current
   behavior).
2. **Range control vs nav** — backing up blindly can back into lava. *Mitigation*: reuse the
   existing flee/kite movement (already nav-aware from Plan 45) rather than raw backward
   intent.
3. **Over-fitting to q3-major** again (Plan 45's plateau). *Mitigation*: the deliverable is
   the mechanism + honest A/B, not a guaranteed win; stop-loss after 2 tuning rounds.

## Verification Checklist

- [ ] T1: enemy `held_weapon` populated live against another bot (log proof) + unit tests; commit.
- [ ] T2: fixed `IDEAL_DIST`/`BACKUP_DIST` gone from `main`; band table unit-tested; commit.
- [ ] T3: `matchup_score` unit tests (blaster-vs-rail@600 loses, ssg@100 wins); kite/flee
      trigger wired; commit.
- [ ] T4: A/B recorded in `brain_notes.md`; main kd > 0.68 or findings documented; commit.
- [ ] fmt + clippy(-D warnings) + tests green before each commit.
