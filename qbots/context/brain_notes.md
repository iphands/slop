# Brain development notes (running log)
# Started: 2026-06-18
#
# Append a dated section on EVERY brain plan (23–33) and any ad-hoc brain change.
# Format mirrors context/map_errors.notes.log.md: observed behavior, hypotheses,
# what was tried, outcome. Newest at the bottom. Keep entries dense, no fluff.

## 2026-06-18 — Plan 23: brain plugin core (trait Brain)
- Goal: introduce `trait Brain` + `BrainKind` factory; existing brain implements it; zero behavior change.
- Seam shape: `brain::brains::core` holds `trait Brain` + bundled I/O (`BrainContext<'a>`,
  `BrainOutput`, `BrainConfig`, `BrainMap`); `brains::mod` holds `BrainKind` enum +
  `build_brain(kind, skill, cfg) -> Box<dyn Brain + Send>` (mirrors `NavMode`/`build_navigator`).
- Seam shipped: `trait Brain` (set_map/tick/on_kill/on_death/heatmap_weights/status) in
  `brains::core`; `BrainKind::Main` + `build_brain` factory in `brains::mod`. Root `brain::Brain`
  export flipped from the concrete struct to the **trait**; the binary owns `Box<dyn Brain + Send>`.
- `tick` body is **byte-identical** to Plan 22 — the only change is the signature: it now
  destructures `BrainContext { view, nav, cm, dt, ticks }` / `set_map` destructures `BrainMap`.
  Pure adapter; no decision logic touched.
- Cosmetic change: periodic log used `brain.behavior()` (`Debug` of `BehaviorState`) → now
  `brain.status()` (`&str` label: roam/hunt/engage/flee/pickup). `behavior()` kept `#[cfg(test)]`
  for the typed-state unit test. Core stays decoupled from `BehaviorState` (main-specific).
- Surprise / process note: a trait extraction that changes signatures can't keep the binary
  green between "impl trait" and "update caller", so T3 folded `bot_task`'s 3 call sites into the
  same commit (inseparable). Used `use brain::brains::core::Brain as _` transiently in T3, then
  T5 made the root `Brain` the trait and switched construction to `build_brain`.
- Verification: `cargo build`/`clippy -D warnings`/`cargo test` (all 18 test binaries) green;
  `BrainConfig::default` combat-on/no-override + `build_brain(Main).status()=="roam"` asserted.
  **Live `connect-one`/`spawn-to-*` NOT run this session — server `noir40.lan` unreachable.**
  Behaviour-preserving by construction (adapter only); flag a live A/B once a server is up.

## 2026-06-18 — Plan 24: `main` brain plugin (relocate + prove the seam)
- `brain.rs` → `brains/main.rs`, struct `Brain` → `MainBrain` (git tracked as a rename; ~25 of
  ~454 lines touched — all the rename + doc, decision body verbatim). `pub mod brain` dropped;
  root exports now come from `brains::{core,main,sentry}`.
- Added `brains/sentry::SentryBrain` (~50 lines): stationary, combat-only, ignores nav — the
  proof that `trait Brain` is a real seam (a second impl sharing no state with MainBrain).
  `BrainKind::{Main,Sentry}`; `build_brain` dispatches both. `Sentry` is code/test-only until
  Plan 25 adds the `--brain` flag.
- `main` behaviour unchanged by construction (move+rename only). Verification: brain crate 106
  unit tests + sentry's 2 (constructs/labels; no-enemy tick → zero movement) green; workspace
  `build`/`clippy -D warnings`/`fmt` clean. **Live `connect-one`/`spawn-to-*` NOT run — server
  `noir40.lan` unreachable this session** (same as Plan 23). `scenario.rs` deliberately untouched
  (Plan 26 lifts it into `RunTesterBrain`; Plan 22 T4 stays open until then).

## 2026-06-18 — Plan 25: multibrain selection + --navmode rename
- `BrainKind` is now a `clap::ValueEnum` (added clap derive-only dep to the `brain` lib) +
  `brain_tag`. `--brain <main|sentry>` exposed on `connect-one` and `run` (fleet), threaded
  `bot_task`/`bot_supervisor_loop`/`run_single`/`run_fleet` → `build_brain`. Brain and navmode
  are independent: `build_navigator(navmode,…)` and `build_brain(brain,…)` are separate calls,
  no combination special-cased.
- Per-bot config: `[fleet].brain` (Option<String>, serde-default → main) with `Fleet::brain_kind()`
  parsing + warn-fallback; CLI `--brain` overrides config (like `--count`).
- `competition --brains main,sentry` spawns the `{navmodes}×{brains}` cross product; bots named
  `<group>_<i>` where group = `<mode>` (default single-main → board identical to before) or
  `<brain>-<mode>` when brains vary. Scoreboard regrouped by group tag (was mode-only). qport
  blocks are per-group disjoint; max_bots clamp now over `groups = navmodes×brains`.
- Rename: user-facing flag `--mode`→`--navmode`, `--modes`→`--navmodes` (clap `long=` override;
  internal field names `mode`/`modes` and the `NavMode` type/`build_navigator`/`mode_tag` kept).
  Updated CLI help, README, mode_perf.md. Clap gotcha: the value placeholder still renders as
  `<MODE>` (derived from the field name) — cosmetic; flag name is correct.
- DEVIATION: spawn-to-spawn/spawn-to-weapon did NOT get `--brain` (they got `--navmode`).
  `scenario.rs` uses raw nav/steer primitives, not a `Brain`, so a `--brain` there would be a
  no-op until Plan 26 migrates the scenario to `RunTesterBrain` — Plan 26 adds the functional
  spawn-to-* `--brain` (and flips its default to `runtester`). Avoided shipping a dead flag.
- Verification: 18 test binaries green; `--help` shows `--navmode`/`--navmodes`/`--brain`/`--brains`
  and no `--mode`; invalid brain/navmode rejected. Live matrix run pending a server.

## 2026-06-18 — Plan 26: runtester scenario brain (+ goal_override move)
- `BrainContext.goal_override: Option<NavGoal>` added (per-tick goal injection for lazily-resolved
  scenario goals); `BrainConfig.goal_override` dropped (`combat_enabled` stays). MainBrain reads
  `ctx.goal_override` with the same precedence; `bot_task` passes `None`.
- `brains::runtester::RunTesterBrain` (`BrainKind::RunTester`): the scenario's combat-free tick
  lifted **verbatim** (steering `Steering::new(3.0)` / recovery / 8-tick backoff / 7-ray escape
  as fields; `pursue_target_safe` look-ahead; speed_scale throttle). `set_map` is a no-op; goal
  comes from `ctx.goal_override`; `dt` from the harness; never requests a weapon.
- `BrainOutput.intent_forward` added — the recorder's hindered-(`H`)-flag input (`recorder.rs:280`),
  which is the throttled nav-step forward and **0 during recovery/backoff** (distinct from
  `intent.forward`). The brain sets it precisely so the migration preserves recorder telemetry
  exactly; MainBrain reports its final forward, Sentry 0.
- `scenario.rs` now builds a `Box<dyn Brain>` (default `runtester`, combat off) and calls
  `brain.tick`; the inline ~110-line decision block is deleted. **Closes Plan 22 T4 + retires the
  Plan 15 duplication.** `spawn-to-spawn`/`spawn-to-weapon` gain `--brain` (default `runtester`;
  `main` for an A/B of the live brain's pathing). dt/last_serverframe + recorder/goal/exit stay
  in the harness.
- CI gate (T3): 6 deterministic unit tests over a stub `Navigator` + an open `CollisionModel`
  (`half_space`): steers to look-ahead, drives goal_override into nav, jumps on jump-edge,
  speed_scale throttles, no weapon_request, backoff-replans after sustained no-progress. Shared
  `StubNav` lives in `nav_mode.rs`.
- T5 (optional log aggregator) skipped — no logs without a server.
- **T6 ACCEPTANCE (live 6-navmode sweep vs mode_perf.md) NOT run — server `noir40.lan`
  unreachable this session.** The merge bar (determinism tests + workspace build/clippy/fmt/test,
  all green) is met and the lift is verbatim (parity structural), so no regression is *shipped*;
  the live reach-count re-confirmation across astar/navmesh/4×hybrid is the one outstanding item,
  to run when a server is up (gate: each navmode ≥ baseline − 2/16, no quality regression,
  hybrid-hier no panic).

### 2026-06-18 (later) — Plan 26 T6 live acceptance sweep: PASSED
Server bounced + reachable (q2dm1, maxclients=8). Ran all 6 navmodes × {spawn-to-spawn,
spawn-to-weapon RL} with `--brain runtester --count 6`. Reach counts (/6): s2s astar 5,
navmesh 2, fallback 6, race 5, hier 3, segment 4; s2w astar 6*, navmesh 6, fallback 4,
race 6, hier 0, segment 3†. **Zero panics across all 12 runs** (hybrid-hier no-panic gate
holds). Pattern matches `mode_perf.md` baseline; mean speeds grounded (~180–270 u/s).
\* astar s2w varied 3/6→6/6 across two draws (n=6 spawn-draw noise). † segment s2w 0/6 @55s →
3/6 @180s (time-limited). Verbatim lift confirmed faithful — no nav-behaviour change. Plan 26
T6 gate cleared; plan closed.

## 2026-06-19 — Plan 36: Quake 3 character + aggression core (`q3char`)
- New pure module `brain::q3char` — Q3's personality + decision scalars, **no brain/CLI/wire
  yet** (Plan 37 assembles them). Additive only; `MainBrain`/`Sentry`/`RunTester` untouched.
- `Q3Character`: the DM-relevant `chars.h` `[0,1]` traits (attack_skill, reaction_time[0,5]s,
  aim_accuracy, aim_skill, croucher, jumper, walker, aggression, self_preservation,
  vengefulness, camper, easy_fragger, alertness, firethrottle, optional per_weapon_accuracy).
  `from_skill(0..10)` is a monotonic remap (à la Eraser `AdjustRatingsToSkill`): higher skill →
  higher aim/attack/alertness/self-preservation, lower reaction_time + firethrottle. Named
  presets `grunt/major/sarge/camper`; `Default` = `from_skill(5)`.
- `bot_aggression(view, enemy_height_delta)` — `ai_dmq3.c:2199` 0–100 loadout scalar. **PVS
  deviation (distilled §2):** the wire gives no free per-weapon inventory, only the *held*
  weapon + its `STAT_AMMO`. So aggression ranks the **held** weapon's `Weapon::power_tier()`
  (Q2 auto-switches to best on pickup → "held" ≈ "best owned"), gated by the held weapon's
  ammo (`ammo_sufficient`). Tiers `<50` (MG/CG/Blaster, or out of ammo) → 0 (flee). **QUAD
  branch dropped** (quad timer not reliably wire-visible). Health/armor guards + the
  enemy-`>200u`-above bad-angle guard ported faithfully.
- **Held-weapon resolution** (perception change, additive): `SelfState.held_weapon:
  Option<Weapon>` + `held_ammo()`, resolved in `Worldview::from_frame` from the `gunindex`
  view-model configstring via new `Weapon::from_view_model` (`v_rail`→Railgun, `v_shotg2`→SSG
  before `v_shotg`→SG, etc; mapping from `g_items.c` precache). `STAT_AMMO` const un-hidden.
- **`bot_aggression` is NOT scaled by `Q3Character`** — faithful to stock Q3, where AGGRESSION
  biases the *threshold*, not the scalar. The character bias lives in
  `Q3Character::retreat_threshold()` = `50 − (aggression−0.5)·40` clamp `[10,90]`;
  `wants_to_retreat`/`wants_to_chase` compare aggression to that biased threshold (stock Q3 =
  fixed 50). So Sarge presses on a tier-50 shotgun where Camper retreats — same loadout.
- `bot_feeling_bad(view)` — `ai_dmq3.c:2247`: Blaster(=gauntlet)→100, health<40→100,
  Machinegun→90, health<60→80.
- **`BotSkill` coexistence:** `Q3Character` is layered *alongside* the Eraser `BotSkill` axis,
  not replacing it — `MainBrain`/shared combat keep `BotSkill`; the Q3 brain adds Q3 texture.
- 12 unit tests pin the thresholds (rail+slugs→95/chase, MG+50hp→0/retreat, shotgun→50
  boundary, rail-no-ammo→0, hurt-but-armored→press, enemy-above→0, threshold bias spread,
  feeling_bad ladder, from_skill monotonicity, preset spread). All green; clippy/fmt clean.
