# Plan 36 — Quake 3 character + aggression core

> **Status**: pending
> **Created**: 2026-06-19
> **Depends on**: Plan 23 (brain plugin core), Plan 06/07 (combat, skill, weapons)
> **Goal**: Add a reusable `q3char` module — the Q3 `Q3Character` personality (named [0,1]
> traits) plus the loadout-based `bot_aggression` / `bot_feeling_bad` decision scalars,
> adapted to qbots' PVS/wire-visible inventory — with zero behavior change to existing brains.
> **Agent**: ralph-loop

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Port Quake 3's personality + aggression decision layer into a self-contained,
unit-tested `brain::q3char` module that the Plan 37 `Q3Brain` will consume. No brain, no CLI,
no wire changes yet — just the pure decision primitives, so they can be tested in isolation.

**Deliverables**:
1. `crates/brain/src/q3char.rs` — `Q3Character` struct (Q3's DM-relevant `[0,1]` traits),
   `Q3Character::from_skill(skill: SkillLevel)` preset mapping, and named presets.
2. `bot_aggression(view: &Worldview, ch: &Q3Character) -> f32` (0–100) +
   `bot_feeling_bad(view) -> f32`, mirroring `ai_dmq3.c:2199/2247`, reading only
   wire-visible playerstate (held weapon + ammo + health + armor).
3. `Weapon::power_tier()` in `weapons.rs` (BFG>RAIL>RL/HB>GL>SSG/SG>MG/CG>blaster) so the
   aggression scalar ranks the held weapon.
4. `wants_to_retreat`/`wants_to_chase` helpers (aggression vs a character-biased threshold).
5. Unit tests pinning the Q3 thresholds (railgun+slugs→press, MG+low-health→flee, etc.).

**Estimated effort**: Small–Medium (half day).

---

## Context

`context/distilled/quake3.md` §2–3 is the authoritative research distillation — read it first.

### Why a separate "core" plan (mirrors Plan 23)

Plan 23 extracted the `trait Brain` seam *before* adding new brains; this plan extracts the
Q3 **decision primitives** before the `Q3Brain` (Plan 37) wires them into an FSM. Keeping
`bot_aggression` and `Q3Character` as pure functions/data means we can unit-test the exact
Q3 thresholds against synthetic `Worldview`s with no server, no nav, no FSM. Plan 37 then
becomes "assemble the node FSM around tested primitives."

### Key Facts (from `vendor/Quake-III-Arena`)

- **`BotAggression`** (`code/game/ai_dmq3.c:2199`): 0–100 from QUAD/health/armor/best-weapon.
  Threshold **50** gates retreat (`<50`) and chase (`>50`) (`:2268`, `:2321`).
- **`BotFeelingBad`** (`:2247`): gauntlet/health<40 →100, MG→90, health<60→80.
- **Characteristics** (`code/game/chars.h`): the named `[0,1]` traits. DM-relevant set in
  distilled §3 (ATTACK_SKILL, REACTIONTIME, AIM_ACCURACY/SKILL, CROUCHER, JUMPER, WALKER,
  AGGRESSION, SELFPRESERVATION, VENGEFULNESS, CAMPER, EASY_FRAGGER, ALERTNESS, FIRETHROTTLE).

### PVS / wire constraint (critical — see distilled §2)

Q3 reads a full server-side `inventory[]`. qbots sees only `SelfState { health, armor, frags,
ammo:[i32;32], weapon, flags }` (`perception.rs`) — i.e. the **held** weapon and *its* ammo
(Q2 `STAT_AMMO`), not a free per-weapon inventory. So `bot_aggression` ranks the **currently
held** weapon's `power_tier()` (Q2 auto-switches to best-on-pickup, so "held" ≈ "best owned"),
not Q3's scan of all owned weapons. Document this deviation in the module docs + brain_notes.

### Why not just extend `BotSkill`?

`BotSkill`/`Ratings` is the **Eraser** axis (1–5 accuracy/combat/aggression + 0–10 master +
auto-skill drift) used by the shared `combat.rs`/`aim.rs` and `MainBrain`. Q3's personality is
a *different shape* (named [0,1] traits, per-weapon accuracy, firethrottle/alertness texture).
Adding `Q3Character` alongside (not replacing) `BotSkill` keeps `MainBrain` byte-identical and
lets the Q3 brain reuse the shared combat modules while layering Q3 texture on top.

---

## Step-by-Step Tasks

### T1: `Weapon::power_tier()` in `weapons.rs`

**File**: `crates/brain/src/weapons.rs`

**What to do**: Add a `pub fn power_tier(&self) -> u8` (and a doc comment citing
`ai_dmq3.c:2199`) ranking Q2 weapons by the Q3 aggression tiers, mapped per distilled §2:
BFG=100-tier, Railgun=95, Hyperblaster=90, RocketLauncher=90, GrenadeLauncher=80,
SuperShotgun/Shotgun=50, Machinegun/Chaingun=25, Blaster/none=0. Return the *aggression score*
the held weapon contributes (so `bot_aggression` can read it directly). Unit-test the ordering.

**Commit**: `task(T1): Weapon::power_tier ranks held weapon for Q3 aggression`

### T2: `q3char.rs` — `Q3Character` struct + presets

**File**: `crates/brain/src/q3char.rs` (new); register `pub mod q3char;` in `lib.rs`.

**What to do**: Define `Q3Character` with the DM-relevant `[0,1]` fields (distilled §3 table):
`attack_skill, reaction_time (secs, [0,5]), aim_accuracy, aim_skill, croucher, jumper, walker,
aggression, self_preservation, vengefulness, camper, easy_fragger, alertness, firethrottle`,
plus an optional `per_weapon_accuracy: [f32; N]` (default to `aim_accuracy`). Provide:
- `Q3Character::from_skill(skill: SkillLevel) -> Self` — a monotonic mapping (à la Eraser's
  `AdjustRatingsToSkill`): higher skill → higher aim_skill/aim_accuracy/attack_skill, lower
  reaction_time, lower firethrottle (less spray). Document the formula.
- Named presets as `const fn`/associated fns: `grunt()` (low skill, high firethrottle spray),
  `major()` (high aim_skill, low firethrottle, precise), `sarge()` (high aggression + jumper),
  `camper()` (high camper/alertness, low aggression). Reference value sets:
  `vendor/Quake-III-Arena/.../bots/*.c` (read for plausible numbers; do not commit them).
- `Default` = balanced mid (≈ `from_skill(5)`).

**Commit**: `task(T2): Q3Character personality struct + skill mapping + presets`

### T3: `bot_aggression` / `bot_feeling_bad` / retreat-chase helpers

**File**: `crates/brain/src/q3char.rs`

**What to do**: Port `ai_dmq3.c:2199/2247/2268/2321` reading only `view.self_state()`:
```rust
/// 0–100 aggression (ai_dmq3.c:2199), adapted to wire-visible inventory:
/// ranks the HELD weapon (power_tier) gated by health/armor; quad branch only if observable.
pub fn bot_aggression(view: &Worldview, ch: &Q3Character) -> f32 { … }
pub fn bot_feeling_bad(view: &Worldview) -> f32 { … }            // ai_dmq3.c:2247
/// Character-biased threshold: base 50, shifted by (aggression-0.5)*40 so a high-aggression
/// character presses sooner (distilled §3 note — stock Q3's threshold is fixed 50).
pub fn wants_to_retreat(view: &Worldview, ch: &Q3Character) -> bool { … }
pub fn wants_to_chase(view: &Worldview, ch: &Q3Character) -> bool { … }
```
Keep the height-delta branch optional (needs an enemy origin — accept an `Option<f32>`
enemy_height_delta arg or a follow-up; document if deferred).

**Commit**: `task(T3): bot_aggression/feeling_bad + retreat/chase thresholds`

### T4: Unit tests pinning the Q3 thresholds

**File**: `crates/brain/src/q3char.rs` (`#[cfg(test)]`)

**What to do**: Build synthetic `Worldview`s (as `fsm.rs`/`sentry.rs` tests do — `Frame` +
`ConfigStrings`) and assert:
- railgun held + slugs>5 + full health → `bot_aggression ≈ 95`, `wants_to_chase`.
- machinegun + health 50 → aggression 0, `wants_to_retreat`.
- health 90 + only shotgun → aggression 50 (boundary).
- `from_skill` monotonicity (skill 9 vs skill 1: higher aim_skill, lower reaction_time).
- preset sanity (`grunt().firethrottle > major().firethrottle`, etc.).

**Commit**: `task(T4): q3char threshold + preset unit tests`

### T5: Brain-notes entry + module docs

**Files**: `context/brain_notes.md`, module-level `//!` docs in `q3char.rs`.

**What to do**: Append a dated Plan 36 section to `brain_notes.md` (running log per the
Plans 23–33 discipline): the PVS inventory deviation (held-weapon proxy), the threshold-bias
choice, and the `BotSkill` vs `Q3Character` coexistence. `///`-doc all public items, citing the
`ai_dmq3.c`/`chars.h` source lines.

**Commit**: `task(T5): brain_notes Plan 36 entry + q3char docs`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/q3char.rs` | new module: `Q3Character`, `bot_aggression`, helpers | P0 |
| `crates/brain/src/weapons.rs` | `Weapon::power_tier()` | P0 |
| `crates/brain/src/lib.rs` | `pub mod q3char;` + re-exports | P0 |
| `context/brain_notes.md` | dated Plan 36 entry | P1 |

---

## Open Questions / Risks

1. **Held-weapon proxy understates aggression** when the bot owns a strong weapon but holds a
   weak one mid-switch. *Mitigation*: Q2 auto-switches on pickup, so this is rare and transient;
   a fuller observed-inventory (mine pickups from prints/obituaries) is a Plan 38 option.
2. **`ammo:[i32;32]` indexing** — confirm the index→weapon mapping in `perception.rs`/wire is
   per-weapon or only STAT_AMMO (current). *Mitigation*: read `playerstate.rs` `stats[]`
   handling during T3; if only current-weapon ammo is reliable, gate on the held weapon's ammo
   only and document it.
3. **Threshold-bias formula is a design choice** (stock Q3 is fixed 50). *Mitigation*: keep it
   a single documented constant; tune live in Plan 38.

---

## Verification Checklist

- [ ] T1: `cargo test -p brain` — `power_tier` ordering test green; clippy clean.
- [ ] T2: `Q3Character::from_skill` + presets compile; `cargo build` zero warnings.
- [ ] T3: `bot_aggression`/`wants_to_*` implemented reading only `Worldview`.
- [ ] T4: threshold tests (rail→press, MG-hurt→flee, shotgun boundary) green.
- [ ] T5: `context/brain_notes.md` has a dated Plan 36 section; all public items `///`-documented.
- [ ] Whole plan: `cargo build` + `cargo clippy -- -D warnings` + `cargo test` + `cargo fmt` all clean; `MainBrain`/`RunTester`/`Sentry` behavior unchanged (no shared code touched beyond the additive `power_tier`).
