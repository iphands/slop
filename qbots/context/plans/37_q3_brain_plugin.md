# Plan 37 — Quake 3 brain plugin (`q3`)

> **Status**: pending
> **Created**: 2026-06-19
> **Depends on**: Plan 36 (q3char core), Plan 24 (`main`/`sentry` plugins), Plan 25 (`--brain`)
> **Goal**: Ship a `Q3Brain` (`BrainKind::Quake3`, CLI `--brain q3`) — Quake 3's node-based
> deathmatch FSM (Fight/Chase/Retreat/NBG) with the Q3 aim/fire model, driven by the Plan 36
> `Q3Character`, wired end-to-end through `build_brain`/CLI/config/competition.
> **Agent**: ralph-loop

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Implement a new `trait Brain` plugin that reproduces Quake 3's deathmatch decision
loop on top of qbots' existing `Navigator`/`world`/`steer`/`recover` — the explicit
node FSM, the aggression-gated retreat/chase, Q3-style enemy selection, and the Q3 predictive
aim + reaction-time + fire-throttle combat model. A fourth selectable brain alongside
`main`/`sentry`/`runtester`.

**Deliverables**:
1. `crates/brain/src/brains/q3/` — `mod.rs` (`Q3Brain` + node FSM), `aim.rs` (Q3 aim/fire),
   `move.rs` (circle-strafe + jump/crouch dodge).
2. `BrainKind::Quake3` (CLI token `q3`) in `brains::mod`; `build_brain` dispatch; `brain_tag`.
3. CLI/config/competition wiring (`--brain q3`, `[fleet].brain = "q3"`, `--brains …,q3`).
4. Deterministic unit tests over a stub `Navigator` (node transitions, reaction gate,
   fire-throttle duty cycle, aggression-gated retreat).
5. A live acceptance run (q2dm1) — a `q3` bot connects, roams, fights, scores frags; an A/B
   frag comparison vs `main` via `competition --brains main,q3`.
6. Dated `context/brain_notes.md` Plan 37 entry.

**Estimated effort**: Large (2–3 days).

---

## Context

`context/distilled/quake3.md` is the authoritative research (read §1, §4–7 before coding).
Plan 36 delivered the decision primitives (`q3char::{Q3Character, bot_aggression,
wants_to_retreat, wants_to_chase}`); this plan assembles the FSM + combat around them.

### Why a new brain, not a `MainBrain` mode

`MainBrain` is the Eraser bot (5-state flat FSM, shared `combat.rs`/`aim.rs`). The Q3 brain's
*value* is the **explicit Fight/Chase/Retreat/NBG node separation gated by `bot_aggression`**
and the **Q3 aim/fire texture** (per-weapon accuracy, reaction-time sight gate, fire-throttle
duty cycle, radial ground-aim, self-preservation abort). Bolting that onto `MainBrain` would
fork its decision body and risk the byte-identical guarantee. The `trait Brain` seam exists
precisely so a second decision philosophy is a *sibling*, not a mode flag — `SentryBrain`
proved it runs; `Q3Brain` is the first *full* alternative.

### Reuse vs reimplement

| Concern | Reuse | Reimplement for Q3 |
|---|---|---|
| Navigation | `Navigator` (inject per tick), `nav.pursue_target`, `smooth_with_cm` | node→goal selection |
| Steering primitives | `steer.rs` (`change_yaw`, `arrive_scale`), `move_ctrl::MovementIntent`, `move_from_world_dir` | circle-strafe cadence, jump/crouch dodge |
| Stuck recovery | `recover::Recovery` (drop in, same as MainBrain) | — |
| LOS / trace | `los::has_los_player`, `cm` traces | — |
| Aim math base | `aim.rs` lead helpers where convenient | per-weapon accuracy, reaction gate, fire-throttle, error model |
| Personality | `q3char` (Plan 36) | — |
| Item goals | `items::best_item_goal` (LTG/NBG proxy) | NBG deadline/range timers |

### Key node-FSM facts (distilled §1)

Node enum: `{ Respawn, SeekLtg, SeekNbg, BattleFight, BattleChase, BattleRetreat, BattleNbg }`.
Transitions are the payload — summarized:
- `SeekLtg`: no enemy → roam to item goal; periodic NBG check (range 150) → `SeekNbg`;
  `BotFindEnemy` → `wants_to_retreat` ? `BattleRetreat` : `BattleFight`.
- `BattleFight`: enemy gone → `SeekLtg`; enemy out of sight → `wants_to_chase` ? `BattleChase`
  : `SeekLtg`; after attacking, `wants_to_retreat` → `BattleRetreat`.
- `BattleChase`: enemy reappears → `BattleFight`; reached last spot / 10s `chase_time` →
  `SeekLtg`; nearby item → `BattleNbg`.
- `BattleRetreat`: `wants_to_chase` flips → `BattleChase`; enemy unseen 4s → `SeekLtg`; moves
  to an item while backing away; grabs nearby items (`BattleNbg`).
- `BattleNbg`: grab transient item; return to prior node.
- Loop guard: per-tick node-switch counter ≤ 50 (`MAX_NODESWITCHES`), else clamp + log.

---

## Step-by-Step Tasks

> Group: T1–T2 build the brain skeleton; T3–T5 the combat model; T6 wiring; T7 tests;
> T8 live acceptance; T9 docs. Commit at each.

### T1: `brains/q3/mod.rs` skeleton + node enum + `trait Brain` impl shell

**File**: `crates/brain/src/brains/q3/mod.rs` (new); `pub mod q3;` in `brains/mod.rs`.

**What to do**: Define `Q3Node` enum, `Q3Brain { ch: Q3Character, node: Q3Node, nav state
(roam_nodes/roam_idx/nav_graph/roam_as_position like MainBrain), per-node timers, combat sub-
state, recovery: Recovery }`. Implement `Brain` with `set_map` (copy MainBrain's), `status()`
returning the node label, `on_kill`/`on_death`, and a `tick` that dispatches on `self.node`.
Start with `SeekLtg` only doing roam-to-goal (port MainBrain's goal ladder) so the bot *walks*
before combat lands. Constructor `Q3Brain::new(ch: Q3Character)`.

**Commit**: `task(T1): Q3Brain skeleton + node enum + roam-only SeekLtg`

### T2: node FSM transitions (Seek/Fight/Chase/Retreat/NBG)

**File**: `crates/brain/src/brains/q3/mod.rs`

**What to do**: Implement the per-node transition logic from distilled §1 using
`q3char::{wants_to_retreat, wants_to_chase}` and an enemy-selection call (T3). Track
`chase_time` (10s), `nbg_time`, `enemyvisible_time` (4s), `check_time` (0.5s NBG poll). Each
battle node drives the injected `Navigator` to the right goal (enemy origin / last-known /
item). Add the per-tick node-switch guard (≤50). Keep aim/fire stubbed (no-op) until T3–T5.

**Commit**: `task(T2): Q3 node FSM transitions (aggression-gated retreat/chase)`

### T3: Q3 enemy selection over `Worldview`

**File**: `crates/brain/src/brains/q3/mod.rs` (or `q3/enemy.rs`)

**What to do**: Port `BotFindEnemy` (`ai_dmq3.c:2929`, distilled §4) over
`view.enemies()` (PVS already limits us): closest-preference, `ALERTNESS`-scaled range gate
(`dist² > (900+alertness·4000)²`), 360° awareness when health dropped this frame or enemy
shooting (else FOV narrows with distance), LOS gate via `los::has_los_player`, the
"sneak past if not in their FOV and wants-retreat" branch. Track `enemysight_time`
(tick-based) for the reaction gate. Health-drop = compare to last tick's `health`.

**Commit**: `task(T3): Q3 enemy selection (alertness range, awareness FOV, LOS)`

### T4: `brains/q3/aim.rs` — Q3 aim model

**File**: `crates/brain/src/brains/q3/aim.rs` (new)

**What to do**: Port `BotAimAtEnemy` (`ai_dmq3.c:3261`, distilled §5): per-weapon
accuracy/skill from `Q3Character`; reaction-time sight gate (`aim_skill>0.95` → don't aim until
sighted > `0.5·reaction_time`); velocity memory sampled every 0.5s + direction-change accuracy
penalty; linear lead for projectile weapons (reuse `aim.rs` lead where possible; replace AAS
exact-predict with constant-velocity extrapolation); radial ground-aim for splash weapons
(`aim_skill>0.6`, trace floor in front via `cm`); the aim-error model (worldspace jitter,
hitscan distance falloff, direction perturbation, weapon-spread comp). Output ideal yaw/pitch.

**Commit**: `task(T4): Q3 per-weapon predictive aim + error model`

### T5: `brains/q3/aim.rs` — fire decision + `move.rs` circle-strafe

**Files**: `crates/brain/src/brains/q3/aim.rs`, `crates/brain/src/brains/q3/move.rs` (new)

**What to do**:
- **Fire** (`BotCheckAttack`, `:3555`, distilled §6): reaction gate, "must be facing aim target
  within FOV" gate, LOS trace unblocked, **fire-throttle duty cycle** (alternating shoot/wait
  windows from `firethrottle`), gauntlet-range, and the **radial self-preservation abort**
  (trace muzzle→impact; if splash radius reaches self and `self_preservation` high, hold fire).
- **Move** (`BotAttackMove`, `:2631`): circle-strafe perpendicular to enemy with random
  strafe-direction flip cadence (`0.4 + (1-attack_skill)*0.2`), ideal-distance band
  (forward/backward blend), random back-up, jump (random<`jumper`) / crouch (random<`croucher`)
  dodge with 1s cooldowns. Reuse `steer.rs`/`move_from_world_dir`; emit `MovementIntent`
  (confirm/`add` crouch support — see Risk 2).

**Commit**: `task(T5): Q3 fire-throttle + self-preservation + circle-strafe dodge`

### T6: wire `BrainKind::Quake3` through factory, CLI, config, competition

**Files**: `brains/mod.rs`, `crates/qbots/src/{main.rs,config.rs,supervisor.rs}`

**What to do**: Add `BrainKind::Quake3` (`#[value(name = "q3")]`), `brain_tag → "q3"`,
`build_brain` arm `Box::new(Q3Brain::new(Q3Character::from_skill(skill.skill)))` (and an
optional preset arg deferred to Plan 38). Verify `--brain q3` works on `connect-one`/`run`,
`[fleet].brain = "q3"` parses, and `competition --brains main,q3` builds the cross product
(group tags already handle >1 brain). Update README/`mode_perf.md` flag docs.

**Commit**: `task(T6): wire --brain q3 through build_brain/CLI/config/competition`

### T7: deterministic unit tests

**Files**: `brains/q3/mod.rs` + `crates/brain/tests/q3_brain.rs`

**What to do**: Using `StubNav` (`nav_mode.rs`) + an open `CollisionModel` (as Plan 26 T3
does): assert node transitions (no-enemy SeekLtg roams; enemy+low-aggression → BattleRetreat;
enemy+high-aggression → BattleFight; enemy-out-of-sight+chase → BattleChase; chase timeout →
SeekLtg), the reaction-time gate (no fire before sight delay), the fire-throttle duty cycle
(fires only within shoot windows), and the node-switch guard. `build_brain(Quake3).status()`
round-trip + `brain_tag`/`ValueEnum` parse tests in `brains/mod.rs`.

**Commit**: `task(T7): Q3Brain deterministic FSM + fire-gate tests`

### T8: live acceptance (q2dm1) + A/B vs main

**File**: `context/brain_notes.md` (record results)

**What to do**: With a server up: `qbots connect-one --brain q3` (verify connect, roam, fight,
no kicks); `qbots run --brain q3 --count 4` (fleet, frags accumulate); `qbots competition
--navmodes astar --brains main,q3 --count 4` for an A/B frag scoreboard. Record reach/frag
counts, any panics, and behavioral notes. Gate: q3 connects + scores ≥1 frag/30s, no kicks,
no panics, frag count within a sane band of `main`.

**Commit**: `task(T8): live q2dm1 acceptance — q3 fights + A/B vs main`

### T9: brain-notes + docs

**Files**: `context/brain_notes.md`, module `//!` docs, README brain list.

**What to do**: Dated Plan 37 `brain_notes.md` entry (node-FSM choices, what was reused vs
reimplemented, AAS-predict→constant-velocity substitution, live A/B results). `///`-doc public
items citing `ai_dmnet.c`/`ai_dmq3.c` lines. Add `q3` to the README/help brain list.

**Commit**: `task(T9): brain_notes Plan 37 entry + q3 docs/README`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/q3/mod.rs` | `Q3Brain` + node FSM + enemy select | P0 |
| `crates/brain/src/brains/q3/aim.rs` | Q3 aim + fire-throttle + self-preservation | P0 |
| `crates/brain/src/brains/q3/move.rs` | circle-strafe + jump/crouch dodge | P0 |
| `crates/brain/src/brains/mod.rs` | `BrainKind::Quake3`, `build_brain`, `brain_tag` | P0 |
| `crates/qbots/src/{main,config,supervisor}.rs` | `--brain q3` wiring | P1 |
| `crates/brain/tests/q3_brain.rs` | deterministic FSM/fire tests | P1 |
| `context/brain_notes.md` | dated Plan 37 entry | P1 |

---

## Open Questions / Risks

1. **AAS exact-predict has no qbots equivalent.** *Mitigation*: use constant-velocity lead
   (already in `aim.rs`) + optional gravity for grenades; the exact-predict path was only for
   `aim_skill>0.8`, so high-skill bots just use better linear lead. Document the substitution.
2. **Crouch in `MovementIntent`.** *Mitigation*: `MovementIntent` has `up`; confirm a crouch
   maps to `up<0` per pmove (`vendor/yquake2` `pm_flags`/`PMF_DUCKED`); add a `crouch()` helper
   if missing. If the wire/pmove makes crouch unreliable as a client, drop croucher to a no-op
   and note it (jumper is the important dodge).
3. **"Enemy is shooting" detection** (drives 360° awareness + invisible-enemy aim). *Mitigation*:
   infer from muzzleflash temp-entities / `EntityState` events if available; conservative
   fallback = unknown→not-shooting (the health-drop branch still gives full awareness when hit).
4. **Node thrash / oscillation** between Fight↔Retreat near the aggression boundary.
   *Mitigation*: the switch-guard caps it; add small hysteresis (e.g. retreat sticks for ≥0.5s)
   if observed live in T8.
5. **Scope creep into CTF/teamplay.** *Mitigation*: DM only — explicitly drop LTG team types,
   chat, grapple, kamikaze (distilled §7).

---

## Verification Checklist

- [ ] T1: `Q3Brain` builds, `status()` labels the node; roam-only SeekLtg walks (stub-nav test).
- [ ] T2: node transitions implemented; switch-guard caps at 50/tick.
- [ ] T3: enemy selection honors alertness range + LOS + awareness FOV (unit test).
- [ ] T4: per-weapon accuracy + reaction gate + lead + error model produce bounded yaw/pitch.
- [ ] T5: fire-throttle duty cycle + self-preservation abort + circle-strafe verified by test.
- [ ] T6: `--brain q3` / `[fleet].brain="q3"` / `--brains main,q3` all work; `--help` shows `q3`.
- [ ] T7: `cargo test -p brain` green incl. `tests/q3_brain.rs`; `ValueEnum`/`brain_tag` round-trip.
- [ ] T8: live q2dm1 — q3 connects, fights, ≥1 frag/30s, no kicks/panics; A/B vs main recorded.
- [ ] T9: `context/brain_notes.md` has a dated Plan 37 entry; public items documented; README lists `q3`.
- [ ] Whole plan: `cargo build`/`clippy -D warnings`/`test`/`fmt` clean; `main`/`sentry`/`runtester` unchanged.
