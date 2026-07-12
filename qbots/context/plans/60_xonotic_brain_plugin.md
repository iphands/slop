# Plan 60 — Xonotic brain plugin (`xon`)

> **Status**: pending
> **Created**: 2026-07-11
> **Depends on**: Plan 59 (xoncore primitives), Plan 46 (traversal), Plan 48/49/50 (hazard + acquisition). (Plan 58's shared `follow_path` was abandoned — see its row in SERIES; `xon` carries its own locomote copy following q3's shape, `q3/mod.rs` `locomote`.)
> **Goal**: A full Xonotic-havocbot-derived brain (`--brain xon`) — goal-stack strategy with rating-driven goal selection, evidence-based re-planning, Xonotic weapon choice + weapon combos, the XonAim dynamical system, and keyboard-emulated movement — sharing ALL traversal/hazard/locomotion infrastructure with the other brains.
> **Agent**: implementation agent (ralph-loop)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Assemble `XonBrain` from the Plan 59 primitives on q3's locomotion shape (own `locomote` copy delegating to the shared Steering/Recovery/TraversalExecutor/hazard modules); wire it as a first-class `BrainKind` and prove it with spawn-to-* scenarios and live competition.

**Deliverables**:
1. `crates/brain/src/brains/xon/mod.rs` — `XonBrain` implementing `trait Brain` (honors `goal_override`, `combat_enabled=false`, `intent_forward`).
2. Goal-stack strategy layer: 7 s/5.5 s rating sessions over `flood_costs` + evidence-based early expiry + progress watchdog + 3 s goal blacklist, using shared `ItemMemory` respawn timers.
3. Xonotic combat: sticky enemy selection (Plan 49 damage-widen parity), far/mid/close weapon lists + `2^rangepreference` + mid-refire weapon combos, XonAim aim/fire, flight-path projectile dodge, 80–100 u keepaway.
4. Full wiring: `BrainKind::Xon` (`xon`/alias `xonotic`), `build_brain`, `brain_code`, connect-one/run/spawn-to-*/competition; `--xonchar` preset selection.
5. Proof: unit tests + spawn-to-spawn/weapon/item reach matrix + live competition vs `mai`/`q3` recorded in `mode_perf.md`.

**Estimated effort**: Large (2–3 days)

## Context

### Key Facts

**Authoritative research: `context/distilled/xonotic.md`** — §1 (architecture), §2 (goal stack/rating), §4 (movement), §5 (aim), §6 (combat), §8 (portability verdicts). Read before each task.

What makes `xon` different from our existing brains (distilled §9): a single smooth objective (`value * rangebias/(rangebias+cost)`) over ALL goal types instead of an FSM picking goal categories; re-planning on *evidence* (item delta vanished, no progress 0.5 s, goal freed) instead of timers alone; aim as a dynamical system; keyboard-quantized movement; weapon combos.

### Reuse vs reimplement (the Plan 37 table, updated)

| Piece | Decision |
|---|---|
| Path following | **Copy q3's `locomote` shape** (Plan 58 abandoned) — steer/creep/recovery/jump/traverse in the proven order, delegating to the shared modules |
| Traversal (swim/ride/ladder/lift/air) | **Reuse** `TraversalExecutor` (mandatory, Plan 46/31/32) |
| Hazard gates + lava override | **Reuse** `hazard::*` (Plan 48/50); lava-escape caller glue copied from q3 |
| Stuck recovery | **Reuse** `Recovery`/`StuckDetector`; Xonotic's 0.5 s progress watchdog is ADDITIONAL, at the goal level |
| Item respawn inference | **Reuse** `items::ItemMemory` (Plan 30) — it implements distilled §2 "item timing" already |
| Goal selection | **Reimplement** — the rating session IS the brain (xoncore::rating + flood_costs) |
| Enemy selection | **Reimplement** (sticky 2 s/4 s, nearest-visible) but with the Plan 49 contract: health-drop widens acquisition to full sphere ~3 s |
| Aim/fire | **Reimplement** via `xoncore::aim::XonAim` (q3 precedent: own aim, not `CombatDriver`) |
| Weapon choice | **Reimplement** (far/mid/close lists + combos); consult `weapons.rs` tables for ranges/refire |
| Dodge | **Reimplement** flight-path perpendicular dodge (distilled §4) over PVS projectiles; `DangerDriver` (Eraser) stays main's |
| Movement texture | **Reimplement** keyboard quantizer application + low-skill overshoot stop; bunnyhop **deferred** (Q2 physics recalibration, distilled §8) |

### Scenario-harness contract (from the seam audit)

`spawn-to-*` builds the brain with `BrainConfig{combat_enabled:false}` and injects `goal_override: Some(NavGoal::Position(goal))` per tick. `XonBrain::tick` MUST: (a) when `goal_override` is set, skip the strategy layer entirely and path-follow to it (q3 precedent `q3/mod.rs:876-879`); (b) never fight when `combat_enabled=false`; (c) populate `intent_forward` for the recorder's hindered flag.

## Step-by-Step Tasks

### T1: skeleton + full wiring (walks before it fights)

**Files**: `crates/brain/src/brains/xon/mod.rs` (new), `crates/brain/src/brains/mod.rs`, `crates/qbots/src/main.rs`, `crates/qbots/src/supervisor.rs`, `crates/qbots/src/config.rs`

**What to do**: `XonBrain { skill: XonSkill, steering, recovery, traverse, goals: XonGoals (stub), aim: XonAim, keyboard: KeyboardEmu, rng: SmallRng, ... }`. Tick v1: honor `goal_override` → own `locomote` (q3's shape: pursue_target_safe → change_yaw → creep_scale → gates → recovery w/ safe_strafe_dir → jump-edge → traverse.apply) → the lava-escape override block (q3/mod.rs:916-938 shape); else roam (q3's roam_goal shape). Wire everything NOW so every later task is live-testable: `BrainKind` variant `#[value(name="xon", alias="xonotic")]`, `brain_tag` arm, `build_brain` arm (accepts an `Option<XonCharPreset>` following the q3 `char` pattern — widen the factory param or add a parallel arg, decide at impl and note in tracker), `brain_code` → `"xon"` (supervisor.rs:467-476), competition acceptance (do NOT reject like runtester).

**Verify live**: `connect-one --brain xon` connects, roams, no panics; `spawn-to-spawn --map q2dm1 --brain xon` exit 0.

**Commit**: `task(T1): XonBrain skeleton + BrainKind::Xon wiring (walks on shared locomotion)`

### T2: goal-stack strategy layer

**File**: `crates/brain/src/brains/xon/goals.rs` (new)

**What to do**: `XonGoals` — the distilled §2 machinery:
- **Rating session** every `strategyinterval` 7 s (5.5 s when the goal is an enemy): source node = `nav_graph.nearest(pos)`; ONE `flood_costs_weighted` call; candidates = `BrainMap.items` (value via `xoncore::rating::item_value` × `ItemMemory` availability — item up, or respawning within the skill window) + PVS-visible enemies (`enemy_rating`) + wander annulus fallback (`wander_rating`, last-2 penalty). Winner → `NavGoal` handed to `follow_path`; keep a small goal stack only as bookkeeping (our `Navigator` owns the actual polyline — do NOT duplicate route storage; the "stack" reduces to {final goal, kind, deadline, blacklist}).
- **Evidence-based expiry**: goal item's entity delta observed picked-up while in PVS (`ItemMemory` transition) → expire now; **progress watchdog** — best-ever 2D+Z distance to goal, no improvement 0.5 s → `nav.force_replan()`, second failure → expire + blacklist the goal 3 s (`ignoregoal_timeout`); goal reached → expire.
- **Chase cutover** (distilled §2): enemy goal within 700 u + clear `cm` trace → steer directly (pass as `world_dir_override` hook or let `pursue_target_safe` handle it — decide at impl, note in tracker).
- Fleet amortization: stagger rating sessions by bot ordinal (poor-man's strategy token; the real token needs supervisor plumbing — deferred, noted in Risks).

**Tests**: deterministic — synthetic `BrainMap` + `StubNav`-style flood stub: highest-rated item chosen; expiry on observed pickup; watchdog blacklists after two failures; blacklisted goal not re-chosen within 3 s.

**Commit**: `task(T2): xon goal-stack strategy (rating sessions + evidence expiry + watchdog)`

### T3: enemy selection + weapon choice

**File**: `crates/brain/src/brains/xon/combat.rs` (new)

**What to do**:
- `choose_enemy` (distilled §6): re-scan every 2 s (4 s sticky); sticky keeps the target while `los::has_los_player` within 1000 u, extending 0.5 s per check; pick nearest visible from PVS players. **Plan 49 contract**: health drop → widen acquisition to full sphere for ~3 s (mirror `CombatDriver`'s fix — shot-from-behind bots must engage).
- `choose_weapon` every 0.5 s: far/mid/close priority lists for the Q2 arsenal (thresholds 300/850 u scaled by `2^rangepreference`); dry-weapon exclusion via ammo (Plan 30's `select_best_weapon` logic as reference — reuse its ammo table helpers from `weapons.rs` where they fit); **weapon combos**: if current refire won't finish before `0.4*(4 − 0.3*(skill+weaponskill))`, request the switch mid-refire, then lock 1 s. Respect the Plan 47 weapon-switch-thrash lesson: route requests through the same re-sync + cooldown path main uses (`weapon_request` emission).

**Tests**: sticky window math; sphere-widen on damage; combo triggers exactly per formula; thrash guard (≤1 request/s).

**Commit**: `task(T3): xon enemy selection + weapon lists/combos`

### T4: aim & fire integration

**File**: `crates/brain/src/brains/xon/mod.rs` (+ small additions to `xoncore/aim.rs` if needed)

**What to do**: Feed `XonAim::step` with target pos/vel from frame deltas, `latency` = measured RTT (client exposes ping since Plan 57), per-weapon `shot_speed` from `weapons.rs`. Ballistic arcs (GL): port `findtrajectorywithleading` as a ≤10-iteration gravity-step trace against `cm` (distilled §5.6) — or defer GL to straight-lead if the trace helper exceeds budget (note in tracker). Fire only when `AimCmd.fire` AND `combat_enabled` AND no self-splash (reuse q3's `would_self_splash` idea — promote it from `q3/aim.rs:186` to a shared helper instead of copying).

**Tests**: fire gated by cone; no fire when `combat_enabled=false`; self-splash suppression.

**Commit**: `task(T4): xon aim/fire via XonAim (+ shared self-splash helper)`

### T5: combat movement + dodge + keyboard texture

**File**: `crates/brain/src/brains/xon/mod.rs`, `crates/brain/src/brains/xon/dodge.rs` (new)

**What to do**:
- Keepaway: halt approach at 80 u, pull steer destination out of the target bbox; don't chase >2× maxspeed movers or while swimming (traverse gates already own swimming).
- Flight-path dodge (distilled §4): over PVS projectile entities (velocity from consecutive deltas — `perception.rs` has entity history): `v -= n*(v·n)`, danger = `dodgerating − |v|`, dodge perpendicular; **every dodge dir through `hazard::safe_strafe_dir`** (Plan 48 L2 — non-negotiable).
- Compose Xonotic-style: `dir = normalize(chase + dodge + evade)` then project — but hand the result to `follow_path` via the `world_dir_override` hook so recovery/traverse still gate it.
- **Keyboard quantizer last**: apply `KeyboardEmu::quantize` to the final (fwd, side) before emitting intent; skip while any traverse gate is active (swim/ride/ladder need analog precision). Low-skill overshoot stop (skill+move ≤3, deviation >70° → 0.4–0.6 s halt).

**Tests**: dodge perpendicularity; hazard-gated dodge never points into lava (synthetic CM); keyboard applied only when gates inactive.

**Commit**: `task(T5): xon combat movement + flight-path dodge + keyboard texture`

### T6: unit-test pass + determinism

**File**: `crates/brain/src/brains/xon/*` tests

**What to do**: End-to-end deterministic ticks over `StubNav` + synthetic `Worldview` (Plan 37 T7 pattern): N ticks with a scripted enemy → assert FSM-free invariants (has goal, fires only in cone, weapon request sane); seeded-RNG reproducibility (two runs identical).

**Commit**: `task(T6): xon deterministic brain tests`

### T7: spawn-to-* movement acceptance (the Plan 10 lens)

**Files**: none (verification task; tracker records results)

**What to do**: With a live server per map:
```bash
cargo run -p qbots -- spawn-to-spawn --map q2dm1 --brain xon --count 4
cargo run -p qbots -- spawn-to-weapon railgun --map q2dm1 --brain xon      # swim (S flags)
cargo run -p qbots -- spawn-to-item quaddamage --map q2dm3 --brain xon     # ride (P flags)
cargo run -p qbots -- spawn-to-weapon railgun --instance 1 --map q2dm3 --brain xon  # lift+train
```
Gate: exit 0 on q2dm1 s2s; swim + ride scenarios reach with parity vs `--brain run` control runs from the same session (the shared executor should make this near-automatic — divergence means xon is fighting the gates). Record SUMMARY lines in the tracker.

**Commit**: `task(T7): record xon spawn-to-* acceptance matrix` (tracker/notes only)

### T8: live combat acceptance

**Files**: `context/mode_perf.md`, tracker

**What to do**: (a) `connect-one --brain xon` 2-min watch: acquires on damage-from-behind, weapon combos visible in logs, no thrash (`EVT` counters). (b) `competition --brains mai,q3,xon --count 2` two 5-min runs on q2dm1: xon scores ≥1 frag/30 s, 0 panics/kicks, K/D within the control-spread noise floor of q3's band (use the Plan 47 aggregator; single-run K/D is noise — record mean [min..max]). (c) One 5-min q2dm3 run: 0 drownings, lava deaths ≤ the Plan 50 baseline share. Record in `mode_perf.md`.

**Commit**: `task(T8): xon live competition baseline → mode_perf.md`

### T9: docs + brain_notes + closeout

**Files**: `context/brain_notes.md`, `README`/CLI help (short-code lists), SERIES, plan+tracker

**What to do**: Dated brain_notes entry (what shipped, deliberate drops: bunnyhop/strategy-token/SUPERBOT paths, tuning observations). Help text already auto-lists via `ValueEnum` (Plan 56) — verify. `git mv` to `completed/`; SERIES → done.

**Commit**: `task(T9): xon docs + brain_notes; close Plan 60`

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/brains/xon/{mod,goals,combat,dodge}.rs` | new brain | P0 |
| `crates/brain/src/brains/mod.rs` | BrainKind::Xon + factory | P0 |
| `crates/qbots/src/main.rs` | CLI (`--brain xon`, `--xonchar`) | P0 |
| `crates/qbots/src/supervisor.rs` | `brain_code` "xon" | P0 |
| `crates/qbots/src/config.rs` | `[fleet]` brain/char parse | P1 |
| `crates/brain/src/aim.rs` or new shared mod | promoted `would_self_splash` | P1 |
| `context/mode_perf.md`, `context/brain_notes.md` | baselines + notes | P1 |

## Open Questions / Risks

1. **Rating session cost** — one `flood_costs` per session per bot; 8 bots × ~5k nodes at 7 s cadence is fine, but verify with a timing log line on first live run. *Mitigation*: ordinal stagger (T2); real strategy-token supervisor plumbing deferred to Plan 62 if timing shows need.
2. **`build_brain` signature growth** (persona, q3 char, now xon char). *Mitigation*: consider folding per-brain sub-configs into a `BrainCharacter` enum param at T1; if too invasive, parallel `Option` param and file a follow-up.
3. **Keyboard texture vs recovery** — quantized movement could starve `StuckDetector` progress and trigger false recoveries (Plan 51 territory). *Mitigation*: skip quantization while recovery is active or gates are active; watch `R`-flag rates in T7 logs vs control.
4. **Enemy velocity from deltas is noisy** → aim filter cascade may oscillate. *Mitigation*: Plan 59's stability test at dt=0.1; velocity smoothing over 2–3 frames if needed (note in brain_notes).
5. **Xonotic constants are for a faster game** (Xonotic maxspeed/physics ≠ Q2 320 u/s). Distances (850/300, 700 chase, 1000 sticky) may need Q2 scaling. *Mitigation*: keep them as named consts in one place; T8 tuning notes; full tuning sweep belongs to Plan 62.
6. **Live server availability**. *Mitigation*: T7/T8 mark `blocked` in the tracker if no server; code tasks don't claim live results.

## Verification Checklist

- [ ] T1: `connect-one --brain xon` roams; s2s q2dm1 exit 0; all CLI surfaces list `xon`
- [ ] T2: goal-stack unit tests green (rating choice, evidence expiry, watchdog+blacklist)
- [ ] T3: sticky/widen/combo/thrash-guard tests green
- [ ] T4: fire-cone gating + self-splash tests green; shared helper (no copy of q3's)
- [ ] T5: dodge hazard-gating tests green; keyboard skipped under gates
- [ ] T6: deterministic 2-run reproducibility test green
- [ ] T7: spawn-to-* matrix recorded — s2s exit 0; swim `S` + ride `P` reach with parity vs runtester control
- [ ] T8: competition mean K/D within q3's noise band; ≥1 frag/30 s; 0 panics/kicks/drownings; `mode_perf.md` updated
- [ ] T9: brain_notes dated entry; plan+tracker in `completed/`; SERIES done
- [ ] Whole plan: main/q3/zb2/runtester behavior untouched; zero warnings, clippy clean, fmt, tests green at every commit
