# Plan 59 — Xonotic character + core primitives (`xonchar`, `xoncore`)

> **Status**: pending
> **Created**: 2026-07-11
> **Depends on**: Plan 23 (brain plugin core); Plan 05 (world/NavGraph, for the flood-cost API)
> **Goal**: Port Xonotic havocbot's pure decision primitives — 12-axis additive skill personality, the goal-rating formula + item eval, the aim dynamical system, and keyboard-emulation quantization — as unit-tested modules with NO brain, NO CLI, NO behavior change to existing brains; plus a single-source Dijkstra flood API on `NavGraph`.
> **Agent**: implementation agent (ralph-loop)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Pure, synthetic-input-testable Xonotic primitives (the Plan 36 pattern that de-risked the q3 brain), so Plan 60 becomes "assemble a brain around tested parts".

**Deliverables**:
1. `crates/brain/src/xonchar.rs` — `XonSkill` (global skill + 12 additive axes) + 4 named presets + `char_code`/skin, mirroring `q3char.rs`.
2. `crates/brain/src/xoncore/rating.rs` — `route_rating`, Q2-adapted `item_value`, enemy rating, wander-annulus rating.
3. `crates/brain/src/xoncore/aim.rs` — `XonAim`: error offset, 5-filter anticipation cascade, mouse-think, turn-rate model, fire cone + burst timer.
4. `crates/brain/src/xoncore/keyboard.rs` — movement quantizer (threshold 0.57, skill-gated key vocabulary, analog blend by distance).
5. `world::NavGraph::flood_costs(from) -> Vec<f32>` — single-source travel-cost flood (runtime only, no cache bump).

**Estimated effort**: Medium (1 day)

## Context

### Why a separate core plan (Plan 36 precedent)

Every formula below has exact vendor constants. Porting them inside a live brain means debugging math and FSM wiring simultaneously. As with `q3char`, land them pure first: each function takes plain values / synthetic `Worldview`s, unit tests pin the vendor thresholds, and the brain plan consumes them.

### Key Facts

**Authoritative research: `context/distilled/xonotic.md`** (2026-07-11) — read it first. Sections cited per task. Vendor ground truth: `vendor/xonotic/data/xonotic-data.pk3dir/qcsrc/server/bot/default/`.

- **Rating** (§2): `f *= rangebias / (rangebias + cost)` with both in travel-time seconds (`navigation.qc:1418`); item eval `bot_pickupevalfunc` (weapons `base*(1−0.5*bound(0,arsenal/20000,1))`, ammo `value*min(2,have/need)`, health/armor `base*min(2,amount/current)`); enemy rating `t = bound(0, 1 + hp_diff/150, 3) + max(0,8−skill)*0.05` — enemy hp not on the Q2 wire → parameterize, default 100.
- **Aim** (§5): offset `randomvec()*bound(0,1−0.1*(skill+offsetskill),1)*1.8` resampled 0.2–0.5 s (vertical ×0.7, ×5 fighting); filter poles `.2/.2/.1/.2/.25`, mix `.01/.075/.01/.0375/.01`, blend `bound(0,skill+aimskill,10)*0.1`; mouse-think period `0.5 − 0.05*(skill+thinkskill)`; turn `r = bound(dt, max(15/bound(1,Δang,1000), 2)*dt*(2+(skill+mouseskill)³*0.005−random()), 1)`; fire cone `(1000/(dist−9) − 0.35)` deg scaled `((accurate?1:1.6)+bound(0,(10−(skill+aimskill))*0.3,3))`; burst timer `bound(0.1, 0.5−(skill+aggresskill)*0.05, 0.5)`; lead `target + vel*(shotdelay + dist/shotspeed)`.
- **Keyboard** (§4): threshold 0.57 → {−1,0,1}; key rate `0.05/(sk+kb) + random()*0.025/(skill+kb)`; tiers <1.5 fwd only / <2.5 no diagonals / <4.5 fwd diagonals; blend by `bound(0, dist/250, 1)`; skip when skill ≥ 10.
- **Personality** (§7): 12 additive axes (`keyboard, move, dodge, ping, weapon, aggres, rangepref, aim, offset, mouse, think, ai`), each added to the global skill at its point of use.
- **Flood** (§3): `navigation_markroutes` is a whole-graph single-source flood; rating N candidates then costs O(graph) once instead of N × A*. Our `NavGraph` (`crates/world/src/navgraph.rs`) has per-target `path*` only.
- Determinism: existing brains keep RNG deterministic per-bot (see `skill.rs` / q3 aim); every randomized primitive here takes an injected `&mut impl Rng`-style source so tests seed it.

## Step-by-Step Tasks

### T1: `XonSkill` + presets

**File**: `crates/brain/src/xonchar.rs` (new), `crates/brain/src/lib.rs`

**What to do**: `XonSkill { skill: f32, axes: XonAxes }` with the 12 named axis offsets and accessor methods that mirror the vendor's `skill + axis` sums (e.g. `fn aim(&self) -> f32 { self.skill + self.axes.aim }`, clamped where the vendor clamps). Four presets à la `Q3CharPreset` (q3char.rs:234) — suggested roster (rename freely): `rusher` (+aggres/+move/−offset-care), `sharp` (+aim/+mouse/+rangepref high), `turtle` (−move/+dodge/low rangepref), `noob` (negative axes across the board). `char_code()` (≤3 chars, competition naming) + `skin()`. Doc-comment each axis with the vendor field name + where it's summed (`bot.qc:275-290`).

**Tests**: preset round-trips, axis sums, clamp pins.

**Commit**: `task(T1): add xonchar::XonSkill (12-axis additive personality) + presets`

### T2: rating module

**File**: `crates/brain/src/xoncore/rating.rs` (new), `crates/brain/src/xoncore/mod.rs`

**What to do**:
- `linear_cost(dist_qu, speed) -> f32` and `route_rating(value, rangebias_qu, cost_s, speed) -> f32` (`navigation.qc:1418,1225`).
- `item_value(class: EntityClass, own: &SelfStateView) -> f32` — Q2 arsenal mapping of `bot_pickupevalfunc` (distilled §2): weapon base table (RL/rail high ~8000, SSG/CG mid, blaster 0), ammo `min(2, have/need)`, health/armor `min(2, amount/current)`; take own inventory as a plain struct so tests need no wire types.
- `enemy_rating(my_hp_armor: f32, their_hp_armor_est: f32, skill: f32) -> f32` (`roles.qc:201-213`).
- `wander_rating(rng, last_two: [Option<usize>; 2], node) -> f32` — random 0.5–1.0, ×0.1 for the last two visited (`roles.qc:16`).

**Tests**: pin `route_rating` monotonicity + exact values at cost=0 and cost=rangebias (→ value/2); item table spot pins; enemy formula bounds.

**Commit**: `task(T2): add xoncore::rating (routerating + Q2 item eval + enemy/wander)`

### T3: `XonAim`

**File**: `crates/brain/src/xoncore/aim.rs` (new)

**What to do**: A stateful `XonAim` struct (per-bot): `badaimoffset` + resample clock, the five filter states, `mouse_target` + mouse-think clock, `fire_timer`. API:

```rust
pub struct AimCmd { pub angles: Vec3, pub fire: bool }
impl XonAim {
    /// desired = ideal aim angles at the (lead-corrected) target this tick.
    pub fn step(&mut self, rng: &mut SmallRng, sk: &XonSkill, current: Vec3,
                target_pos: Vec3, target_vel: Vec3, eye: Vec3,
                shot_speed: f32, latency: f32, fighting: bool, dt: f32) -> AimCmd
}
```

Implement the six pipeline stages from distilled §5 in order (offset → filter cascade → mouse-think → turn-rate → fire cone/burst → lead is applied to `target_pos` first). Ballistic `findtrajectorywithleading` is **deferred to Plan 60** (needs CM tracetoss) — `step` takes a pre-computed aim point.

**Tests** (seeded RNG): zero-skill vs skill-10 convergence time on a step-change target (high skill converges faster); stationary target inside fire cone → fire arms and bursts for the timer duration; moving target → lead offset matches `vel*(latency + dist/speed)`; filter cascade is stable (no NaN/oscillation over 10k steps).

**Commit**: `task(T3): add xoncore::aim::XonAim (filter cascade + mouse-think + fire cone)`

### T4: keyboard quantizer

**File**: `crates/brain/src/xoncore/keyboard.rs` (new)

**What to do**: `KeyboardEmu` (per-bot state: current keys + rekey clock). `fn quantize(&mut self, rng, sk: &XonSkill, analog_fwd_side: (f32,f32), dist_to_goal: f32, dt: f32) -> (f32, f32)` implementing threshold/tiers/blend from distilled §4; identity passthrough when `sk.skill >= 10`.

**Tests**: threshold pins (0.56 → 0, 0.58 → 1), tier gating (skill 1 never emits side keys, skill 2 never diagonals), blend limits (dist 0 → pure analog, dist ≥250 → pure keys), rekey clock honors the skill-scaled period.

**Commit**: `task(T4): add xoncore::keyboard (skill-gated usercmd quantizer)`

### T5: `NavGraph::flood_costs`

**File**: `crates/world/src/navgraph.rs`

**What to do**: `pub fn flood_costs(&self, from: usize) -> Vec<f32>` (and `flood_costs_weighted` accepting the same overlay type as `path_weighted`) — Dijkstra from one source to all nodes, returning per-node cost (`f32::INFINITY` unreachable). Pure read-only; **no mapcache VERSION bump** (graph bytes unchanged). This is the O(graph)-once primitive the rating session needs (distilled §2/§3).

**Tests**: synthetic graph pins (line graph costs, unreachable component ∞, overlay changes ordering); parity check `flood_costs(a)[b] ≈ path(a,b) cost` on a random synthetic graph.

**Commit**: `task(T5): add NavGraph::flood_costs single-source Dijkstra`

### T6: docs + brain_notes + closeout

**Files**: `context/brain_notes.md`, module `///` docs, SERIES, plan+tracker

**What to do**: `///` on all public items citing vendor file:line (RULES: load-bearing libs are documented). Dated brain_notes entry: constants pinned, deliberate adaptations (enemy hp default, ballistic deferral). Move plan to `completed/`, SERIES → done.

**Commit**: `task(T6): xoncore docs + brain_notes; close Plan 59`

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/xonchar.rs` | new — 12-axis personality | P0 |
| `crates/brain/src/xoncore/{mod,rating,aim,keyboard}.rs` | new — pure primitives | P0 |
| `crates/world/src/navgraph.rs` | `flood_costs(_weighted)` | P0 |
| `crates/brain/src/lib.rs` | module exports | P1 |
| `context/brain_notes.md` | dated entry | P1 |

## Open Questions / Risks

1. **Angle conventions** — vendor formulas are in degrees over QC `v_angle`; our steering uses radians/yaw-pitch. *Mitigation*: pick ONE unit at the module boundary (degrees, matching vendor constants), document it, convert at the caller; pin a test at a known angle.
2. **Enemy hp unknown on the wire** (Plan 28 pitfall: no VWep either). *Mitigation*: `their_hp_armor_est` parameter, default 100; Plan 60 may later feed a damage-dealt estimate.
3. **Filter cascade stability at large dt** (our brain tick ~0.1 s vs Xonotic 0.05). *Mitigation*: stability unit test at dt=0.1 and dt=0.025; clamp per-step filter input like the vendor's `bound` calls.
4. **Additive-only guarantee** — nothing existing may change. *Mitigation*: its own checklist line; no edits outside new modules + `lib.rs` exports + `navgraph.rs` addition.

## Verification Checklist

- [ ] T1: xonchar tests green; presets distinct; codes ≤3 chars
- [ ] T2: rating pins green incl. cost=rangebias → value/2
- [ ] T3: XonAim seeded-RNG tests green incl. 10k-step stability at dt=0.1
- [ ] T4: keyboard tier/threshold/blend pins green
- [ ] T5: `flood_costs` parity vs `path` on synthetic graphs; no mapcache VERSION change
- [ ] T6: brain_notes entry on disk; `///` docs cite vendor lines; plan in `completed/`
- [ ] Whole plan: main/q3/zb2/runtester byte-untouched (additive only); `cargo build` zero warnings, clippy clean, fmt, tests green at every commit
