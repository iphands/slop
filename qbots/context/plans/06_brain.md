# Plan 06 — Brain (`brain`)

> **Status**: pending
> **Created**: 2026-06-14
> **Depends on**: Plan 05 (world: trace + nav graph)
> **Goal**: A behavior + combat + navigation brain that turns per-frame perception into
> intent, and intent into `Usercmd`s — so a single bot navigates the map, grabs items,
> fights, and scores frags.
> **Agent**: implementation agent (ralph-loop) | sub-agent

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Build `crates/brain` — perception (from Plan 04 snapshots), navigation (over the
Plan 05 nav graph), combat (aim/lead/weapon-select), and a behavior FSM that drives the
movement controller (Plan 04 T4). Algorithms ported from 3ZB2 / Eraser / ACE; mechanisms
rebuilt on our reconstructed world.

**Deliverables**:
1. Perception layer: classify + track visible players, items, projectiles, sounds.
2. Navigation driver: A* over the nav graph + stuck detection/recovery.
3. Movement controller wiring: intent (desired vel + facing) → `Usercmd`.
4. Combat: target selection, lead-aim (projectiles) vs hitscan, weapon select, skill jitter.
5. Behavior FSM: Roam → Hunt → Engage → Flee → Pickup.
6. Per-bot skill/personality config (Eraser `bots.cfg` style).

**Estimated effort**: Large (3+ days)

---

## Context

### Port the ideas, rebuild the mechanisms

The archive bots (3ZB2, Eraser, ACE) are the reference for *behavior*. Their code calls
`gi.trace()`, walks `g_edicts[]`, and links into physics — none of which qbots has. So we
take their algorithms and rebase them on `client`'s snapshots + `world`'s trace/nav graph.

### Inspiration map

| Bot | Source | Borrow |
|-----|--------|--------|
| **3ZB2** | `vendor/3zb2-zigflag/src/bot/{bot,za,func,fire}.c` + `research/bots/3zb2.md` | route-linking, weapon-aware route selection, aiming, CTF AI |
| **Eraser** | `research/bots/eraser.md` (+ extract `bin/Eraser*`) | dynamic map learning, projectile danger avoidance, per-bot skill/personality (`bots.cfg`) |
| **ACE** | `research/bots/ace.md` | dynamic pathing, learning waypoints |

---

## Step-by-Step Tasks

> **RULES.md Rule A/B**: zero warnings, clippy clean, fmt applied, tests green — **commit**
> `task(TN): <desc>` at each boundary.

### T1: Perception layer

**Files**: `crates/brain/Cargo.toml` (deps: `client`, `world`, `glam`), `crates/brain/src/perception.rs`

**What to do**: Each tick, derive a `Worldview` from the latest `client::Snapshot` (Plan 04):
own state (origin/vel/angles/health/armor/ammo/weapon), visible **players** (origin/vel/
weapon, classified enemy/ally), visible **items** (type + origin, from configstring type),
and recent **projectiles/sounds** (rockets/grenades for danger avoidance). Decay stale
contacts (last-known-position) since PVS hides out-of-sight players.

**Commit**: `task(T1): perception layer over client snapshots`

### T2: Navigation driver

**Files**: `crates/brain/src/nav.rs`

**What to do**: Given a goal (item / roam node / enemy last-known-pos), A* over the `world`
nav graph (Plan 05 T4) to the nearest waypoint, then steer toward the next waypoint each
tick via the movement controller. Stuck detection: if origin doesn't advance N ticks, jump,
back off, or re-path. Handle ladders/water/steps using the `world` tracer + `pmove` flags.

**Commit**: `task(T2): navigation driver with stuck recovery`

### T3: Movement-controller wiring

**Files**: `crates/brain/src/move_ctrl.rs`

**What to do**: Convert high-level intent (desired yaw/pitch + forward/side/up + jump/crouch/
attack/weapon) into the `Usercmd` the movement controller (Plan 04 T4) consumes. Reuse Q2
movement constants (max speed, jump velocity, accel) — port from `pmove.c`. Clamp/quantize
angles the way the server expects.

**Commit**: `task(T3): wire intent through movement controller to usercmd`

### T4: Combat — targeting, aim, weapon select

**Files**: `crates/brain/src/combat.rs`, `crates/brain/src/aim.rs`, `crates/brain/src/weapons.rs`

**What to do**:
- **Target select**: nearest enemy in FOV/range weighted by threat + low health.
- **Aim**: hitscan (RG/CG/MG/SSG/Blaster) → aim at current origin + skill jitter; projectile
  (RL/GL) → lead the target by predicted travel time (port the lead math from `bot/fire.c`).
  BFG/hand-grenade special-cased. Add skill-gated reaction delay before firing.
- **Weapon select**: by ammo + range + enemy state (3ZB2 weapon-aware selection); never fire
  RL/GL at point-blank self-range.
- **Danger avoidance** (Eraser): dodge incoming rockets/grenades via `world` trace of their
  predicted path.

**Commit**: `task(T4): combat targeting, lead-aim, weapon selection`

### T5: Behavior FSM

**Files**: `crates/brain/src/fsm.rs`

**What to do**: States: `Roam` (seek high-value items / roam nodes), `Hunt` (move toward
last-known enemy), `Engage` (enemy in range/LOS → combat), `Flee` (low health/armor → seek
health/escape), `Pickup` (nearby item grab). Transitions driven by the `Worldview`. Each
state emits a goal for T2/T3. Keep it small and debuggable (log transitions).

**Commit**: `task(T5): behavior FSM driving nav + combat`

### T6: Skill / personality config

**Files**: `crates/brain/src/skill.rs`

**What to do**: Per-bot params (Eraser `bots.cfg`): aiming accuracy, reaction time (ms),
aggressiveness, preferred weapon, max skill. Scale aim jitter, reaction delay, target-switch
hesitation, and weapon prefs. Loaded from the fleet config (Plan 07).

**Commit**: `task(T6): per-bot skill/personality parameters`

### T7: Verify — a bot that frags

**What to do**: One bot on a populated server (or vs another qbots instance). Assert it
navigates to items, picks them up, engages enemies, and scores frags over a few minutes.
Tune skill params; log FSM transitions. Record tuning findings in `context/distilled.md`.

**Commit**: `task(T7): verify single-bot navigation, pickups, and combat`

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/perception.rs` | Worldview from snapshots | P0 |
| `crates/brain/src/nav.rs` | A* driver + stuck recovery | P0 |
| `crates/brain/src/move_ctrl.rs` | intent → Usercmd | P0 |
| `crates/brain/src/{combat,aim,weapons}.rs` | targeting/aim/weapon | P0 |
| `crates/brain/src/fsm.rs` | behavior FSM | P0 |
| `crates/brain/src/skill.rs` | per-bot skill | P1 |

---

## Open Questions / Risks

1. **PVS-limited perception.** Enemies vanish when they leave our PVS — no omniscience.
   *Mitigation*: T1 keeps last-known-position with decay; FSM treats missing enemies as
   "lost", not "gone". This is a core qbots constraint (AGENTS.md §Domain Knowledge).
2. **Aim vs. server reconciliation.** Our predicted aim can drift from server truth.
   *Mitigation*: T4 re-aims from the latest server frame, not prediction; prediction only
   smooths our own movement.
3. **Movement constants.** Wrong accel/friction = bot that slides or can't reach waypoints.
   *Mitigation*: T3 ports `pmove.c` constants; verify by matching server-traveled distance.
4. **Tuning is unbounded.** Aiming skill / FSM thresholds can soak infinite effort.
   *Mitigation*: ship conservative defaults that "work", tune per the T7 capture only.

---

## Verification Checklist

- [ ] T1: Worldview correctly classifies self/players/items/projectiles each tick.
- [ ] T2: bot pathfinds to an item and reaches it; stuck recovery fires when blocked.
- [ ] T3: intent→Usercmd produces server-recognized movement (no self-slide).
- [ ] T4: hitscan vs projectile aim both connect on a stationary target at range.
- [ ] T5: FSM transitions are logged and sensible (no thrash between Roam/Hunt/Engage).
- [ ] T6: skill 0 misses more than skill 10 on the same target (range check).
- [ ] T7: single bot navigates, picks up items, and scores frags over a multi-minute run.

---

> **⚠️ CRITICAL REMINDERS ⚠️**
> 
> - **COMMIT AT EVERY TASK COMPLETION** — Format: `task(TN): <description>`. DO NOT WAIT!
> - **FIX ALL WARNINGS BEFORE EACH COMMIT** — `cargo clippy -- -D warnings` must pass.
> - **RUN ALL TESTS BEFORE EACH COMMIT** — `cargo test` must pass.
> - **MOVE COMPLETED PLANS TO `completed/` IMMEDIATELY** — When 100% done, `git mv` to `completed/`.
> - **NEVER batch multiple tasks into one commit** — One task per commit, always.
> - **Reread RULES.md BEFORE EACH TASK** — Re-read RULES.md at the start of every task to stay on track.
