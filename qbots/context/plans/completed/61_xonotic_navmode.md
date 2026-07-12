# Plan 61 — Xonotic goal-stack navmode (`xon-goal`, code `xg`)

> **Status**: done (2026-07-11)
> **Created**: 2026-07-11
> **Depends on**: Plan 20 (hybrid navmode pattern), Plan 08 (risk overlay seam), Plan 59 (flood/rating primitives)
> **Goal**: A new `--navmode xg` Navigator that wraps the proven A* driver with Xonotic's route texture — travel-time edge costs (water/fall aware), a 0.25 s-refresh danger field from PVS-observed projectiles/players baked into path costs, shorten-path chase cutover, and a 0.5 s goal-progress watchdog — available to EVERY brain.
> **Agent**: implementation agent (ralph-loop)

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

## TL;DR

**What**: Port the *navigation-layer* half of havocbot (edge-cost model + danger field + route repair) as a wrapping `Navigator`, orthogonal to the `xon` brain — any brain × `xg`, per the Plan 25 axes.

**Deliverables**:
1. `crates/brain/src/xonnav.rs` — `XonNavDriver` wrapping `NavigationDriver` (the Plan 20 "delegate, not rewrite" pattern).
2. Travel-time cost transform: Walk = dist/speed, Swim edges ×(1/0.7), fall-dominant jump-downs = free-fall time — as a **runtime overlay** (no mapcache bump).
3. Danger overlay: per-node additive cost from PVS-visible projectiles/players, `linearcost(dodgerating) − traveltime(obj→node)`, refreshed every 0.25 s, decayed on stale.
4. Route repair: shorten-path direct-chase cutover (≤700 u + clear hull trace) and goal-progress watchdog (0.5 s → replan, twice → `goal_abandoned`).
5. Wiring (`NavMode::XonGoal`, code `xg`) + spawn-to-* verification vs the `as` control + `mode_perf.md` section.

**Estimated effort**: Medium–Large (1–2 days)

## Context

### Why a navmode and not just brain behavior

Plan 25 made brain × navmode orthogonal axes. The Xonotic *strategy* (what to want) lands in Plan 60's brain; the Xonotic *route texture* (what a path costs and when to repair it) belongs at the Navigator seam so `mai`/`q3`/`zb2` can use it too, and so it can be A/B'd against `as` with the same brain — the clean experiment the competition matrix is built for.

### Key Facts

- Research: `context/distilled/xonotic.md` §2 (touch-pop/shorten/watchdog), §3 (cost model + danger field), §8 (verdicts).
- Cost model (`waypoints.qc:1010-1060`): travel-time seconds; water ×(1/0.7); falls `max(xy_cost, sqrt(height/(gravity/2)))`. Bunnyhop ×1.25 and crouch ×0.5 are **skipped** (no Q2 bhop yet; no crouch links in our graph).
- Danger field (`navigation.qc:1874-1906`): additive per-node `dmg`, summed into relaxation (`navigation.qc:1134`). Our seam: `Navigator::set_risk_overlay` (nav_mode.rs:61) + `NavGraph::path_weighted` already price node overlays — the danger overlay is a *second* source with its own refresh cadence.
- **Overlay collision (pre-identified)**: bot_task feeds the heatmap overlay via `set_risk_overlay` when a brain opts in (`heatmap_weights()`, main only). `XonNavDriver` must **compose** (sum) the externally-set overlay with its internal danger field, not overwrite it.
- Plan 20's shape: hybrids own sub-drivers and delegate per trait call; `build_navigator` (main.rs:84-116) is the single factory; both dispatch sites (bot_task main.rs:1026-1036, scenario scenario.rs:365-369) go through it.
- The Navigator sees no `Worldview` in its trait API — danger-source positions must be **pushed in**: add a defaulted trait method `note_dangers(&mut self, dangers: &[DangerSource])` (no-op default, like `set_risk_overlay`) and call it from the brains' shared locomotion stage or bot_task. Keep `DangerSource { pos, vel, rating }` in `brain`.

## Step-by-Step Tasks

### T1: scaffold — passthrough parity

**File**: `crates/brain/src/xonnav.rs` (new)

**What to do**: `XonNavDriver { inner: NavigationDriver, danger: DangerField (empty), watchdog: GoalWatchdog (inert), ... }` delegating every `Navigator` method 1:1. Unit test: identical `pursue_target`/`update` behavior vs a bare `NavigationDriver` on a synthetic graph (parity pin, the Plan 20 T1 idea).

**Commit**: `task(T1): XonNavDriver passthrough scaffold + parity test`

### T2: travel-time edge costs

**Files**: `crates/brain/src/xonnav.rs`, `crates/world/src/navgraph.rs` (only if the overlay seam needs an edge-kind-aware variant)

**What to do**: Compute a per-node (or per-edge, if the existing weighted API supports it — check `path_weighted`'s overlay shape first) multiplier table at `set_map` time from static edge kinds: Swim nodes ×(1/0.7); jump-down edges whose height dominates → cost = free-fall time (distilled §3 formula, gravity 800). Feed through the weighted-path seam on every replan. **No mapcache change** — this is runtime pricing only.

**Tests**: synthetic graph with a wet shortcut vs dry detour — `xg` prefers dry when time-equivalent; fall-edge cost pins.

**Commit**: `task(T2): xg travel-time edge cost transform`

### T3: danger field

**File**: `crates/brain/src/xonnav.rs`

**What to do**: `note_dangers` (new defaulted trait method on `Navigator` + impl): store sources; every 0.25 s rebuild per-node danger = Σ over sources of `max(0, linearcost(rating) − traveltime(src→node))` within a radius cap (vendor uses LOS; approximate with radius + optional cm trace for the top-K nodes — measure first). Sources decay after 0.5 s unseen (PVS honesty). Compose: effective overlay = external `set_risk_overlay` values + danger field, summed at query time. Trigger a replan when the current path's total danger jumps past a threshold (avoid replanning every 0.25 s).

**Danger ratings** (Q2): rocket ~ RL splash 120, grenades 120, enemies ~60 (vendor `bot_dodgerating` analog — named consts, tune in T6). Wire the `note_dangers` call: simplest correct site is the shared locomotion stage (Plan 58) or bot_task's per-frame loop next to the heatmap feed (main.rs:1171-1195) — decide at impl, note in tracker.

**Tests**: synthetic — a rocket parked on the short path reroutes the bot around it; field decays to zero after sources vanish; external overlay still honored (sum, not overwrite).

**Commit**: `task(T3): xg PVS danger field composed into path costs`

### T4: route repair — chase cutover + progress watchdog

**File**: `crates/brain/src/xonnav.rs`

**What to do**:
- **Shorten-path cutover** (0.25 s cadence): goal is `NavGoal::Entity`/`Position` within 700 u AND a hull trace (`cm.trace`, `MASK_SOLID`, standing hull) from pos to goal is clear AND height diff ≤ STEP — then `pursue_target` returns the goal directly and the polyline is dropped; re-acquire a route when the condition breaks.
- **Progress watchdog**: track best-ever 2D + Z distance to the *final* goal; no improvement for 0.5 s → `force_replan()`; second consecutive failure → report via `goal_abandoned()` (nav_mode.rs:63 — the seam brains already poll) so the brain re-rates.
- Preserve `speed_scale`, jump/swim/ride edge passthrough — traversal executor compatibility is non-negotiable (gates read `current_edge_is_*`).

**Tests**: cutover engages only under all three conditions (synthetic CM with a wall proves the trace gate); watchdog abandons after 2×0.5 s stalls; ride/swim edge flags still surface through the wrapper.

**Commit**: `task(T4): xg chase cutover + goal-progress watchdog`

### T5: wiring

**Files**: `crates/qbots/src/main.rs`, `crates/qbots/src/supervisor.rs`

**What to do**: `NavMode::XonGoal` `#[value(name="xg", alias="xon-goal")]`; `needs_mesh` → false; `build_navigator` arm; `mode_tag`/`mode_code` (supervisor.rs:440-463); competition `value_variants` picks it up automatically (main.rs:2084-2088) — verify `--navmodes` help lists it (Plan 56 value_enum).

**Verify live**: `connect-one --navmode xg` runs; `spawn-to-spawn --map q2dm1 --navmode xg` exit 0.

**Commit**: `task(T5): wire NavMode::XonGoal (xg) through CLI/factory/competition`

### T6: verification sweep + docs

**Files**: `context/mode_perf.md`, `context/brain_notes.md`, SERIES, plan+tracker

**What to do**: With a live server:
```bash
# reach parity vs the as control (same session, same map)
cargo run -p qbots -- spawn-to-spawn  --map q2dm1 --navmode xg --count 4
cargo run -p qbots -- spawn-to-weapon railgun --map q2dm1 --navmode xg   # swim
cargo run -p qbots -- spawn-to-item quaddamage --map q2dm3 --navmode xg  # ride
cargo run -p qbots -- spawn-to-weapon railgun --instance 1 --map q2dm3 --navmode xg
# behavior A/B: same brain, different navmode
cargo run -p qbots -- competition --brains q3 --navmodes as,xg --count 2   # 5 min
```
Gates: reach parity with `as` on all four scenarios (the wrapper must never lose capability); competition shows xg within noise of `as` K/D (danger-avoidance may *help*; it must not hurt reach). New `mode_perf.md` section (seven-navmode table now); dated brain_notes entry (danger-rating consts chosen, cutover observations). `git mv` to `completed/`, SERIES → done.

**Commit**: `task(T6): xg live sweep → mode_perf.md; close Plan 61`

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `crates/brain/src/xonnav.rs` | new wrapping Navigator | P0 |
| `crates/brain/src/nav_mode.rs` | defaulted `note_dangers` + `DangerSource` | P0 |
| `crates/qbots/src/main.rs` | NavMode::XonGoal + factory | P0 |
| `crates/qbots/src/supervisor.rs` | mode_tag/mode_code | P1 |
| `crates/world/src/navgraph.rs` | only if weighted API needs edge-kind variant | P1 |
| `context/mode_perf.md` | xg section | P1 |

## Open Questions / Risks

1. **Overlay composition** — heatmap (external) + danger (internal) double-pricing hot lanes could over-avoid. *Mitigation*: sum with independent scales; only `main` feeds heatmap today, and q3/zb2/xon default (0,0) — A/B with q3 keeps the experiment clean.
2. **Danger-field CPU** — Σ sources × nodes at 4 Hz. *Mitigation*: radius-cap node set via the graph's spatial index; measure with a debug timing line before optimizing.
3. **Watchdog vs TraversalExecutor** — waiting for a lift/plat looks like "no progress" and must NOT trip the watchdog (Plan 31's WaitClear standoff is intentional stillness). *Mitigation*: suspend the watchdog while any traverse-relevant edge (`current_edge_is_ride/swim`, ladder) is active — same suspension contract recovery uses; explicit unit test.
4. **Cutover through hazards** — a clear MASK_SOLID trace can still cross a lava gap. *Mitigation*: reuse the Plan 48 floor-probe idea (`hazard::dir_is_hazardous`) on the cutover segment; test with a synthetic lava strip.
5. **`note_dangers` call-site** spans crates. *Mitigation*: defaulted no-op trait method keeps every other Navigator untouched; wire at one site only.

## Verification Checklist

- [ ] T1: passthrough parity test green (xg ≡ as when features inert)
- [ ] T2: cost-transform pins green; no mapcache VERSION change
- [ ] T3: reroute-around-rocket + decay + overlay-sum tests green
- [ ] T4: cutover gate + watchdog + ride/swim passthrough tests green (incl. lift-wait suspension)
- [ ] T5: `--navmode xg` on every CLI surface; s2s q2dm1 exit 0
- [ ] T6: 4-scenario reach parity vs `as`; competition A/B within noise; `mode_perf.md` + brain_notes updated; plan in `completed/`
- [ ] Whole plan: `as`/`nm`/hybrids byte-untouched; zero warnings, clippy clean, fmt, tests green at every commit
