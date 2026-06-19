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

## 2026-06-19 — Plan 37: Quake 3 brain plugin (`--brain q3`)
- New `brains::q3` (`BrainKind::Quake3`, CLI `q3`) — a full sibling brain to `MainBrain`/`Sentry`/
  `RunTester`, assembled from the Plan 36 `q3char` primitives. `MainBrain` untouched.
- **Node FSM** (`mod.rs`, `ai_dmnet.c`): `SeekLtg/SeekNbg/BattleFight/BattleChase/BattleRetreat/
  BattleNbg` driven by per-tick transitions gated by `q3char::wants_to_retreat/chase` (the
  aggression scalar). `BattleRetreat` (disengage when out-gunned/hurt) is the new behavior vs
  MainBrain's flat FSM. Timers in absolute seconds (driven by `dt`): chase 10s, retreat-unseen 4s,
  NBG 5s deadline + 0.5s poll. Per-tick switch guard ≤50 (`MAX_NODESWITCHES`).
- **Enemy selection** (`BotFindEnemy`): over PVS-limited `view.enemies()` — alertness range gate
  `(900+alertness·4000)`, awareness FOV (360° the frame health drops, else 150°→90° by distance),
  LOS trace, closest-preference, and the **sneak-past** branch (skip a distant non-facing enemy
  we'd rather not fight). `enemy_first_seen` set to now (fresh) / now−2 (upgrade) for the reaction
  gate.
- **Aim** (`q3/aim.rs`, `BotAimAtEnemy`): per-weapon accuracy (`Q3Character::weapon_accuracy`),
  reaction-time sight gate (high-skill bots don't aim early), 0.5s velocity memory + direction-
  change penalty, radial ground-aim for splash weapons (`trace_floor`), and the worldspace +
  angular + hitscan-falloff error model. **AAS exact-predict → constant-velocity lead** via the
  shared `crate::aim::aim_direction` (the exact path was only `aim_skill>0.8`, so high skill just
  gets a better linear lead). `would_self_splash` for the self-preservation abort.
- **Fire** (`BotCheckAttack`): reaction gate + 0.1s weapon-change lockout + facing-FOV gate (120°
  close / 50° far) + LOS + range sanity + self-preservation splash abort + **fire-throttle duty
  cycle** (`random()>firethrottle` → wait `ft`s, else shoot `1−ft`s).
- **Move** (`q3/move.rs`, `BotAttackMove`): circle-strafe perpendicular with a skill-tuned random
  flip cadence (`0.4+(1−attack_skill)·0.2`s, flip on roll>0.935), ideal-distance band (300±100),
  random back-up, jump (roll<jumper) / crouch (roll<croucher) dodge with 1s cooldowns. Retreat
  biases the move backward while still firing. **CROUCHER is best-effort**: `MovementIntent.crouch`
  is a controller no-op today (the wire/pmove duck path isn't wired), so jumper is the real dodge —
  documented deferral (Plan 37 Risk 2).
- **"Enemy is shooting"** is not wire-observable → treated as not-shooting (Risk 3); the health-drop
  branch still grants 360° awareness when we actually take damage.
- **Held weapon for combat** reads the wire-resolved `view.held_weapon` for *aggression*, but
  weapon *switching* still tracks an optimistic `held_weapon` (`use <name>`; server grants if owned).
- **T8 LIVE ACCEPTANCE (q2dm1, noir.lan, 2026-06-19): PASSED.** First A/B `main,q3` ×3 over 130s:
  q3 scored **0** — root cause: Q2 starts everyone on the **blaster** (tier 0 → aggression 0 →
  permanent `BattleRetreat` → bot backs out of blaster range → never frags). Fix: **blaster-floor**
  — in `bot_aggression`, a *healthy* bot holding the blaster floors at 50 (engage-worthy; the
  blaster is Q2's infinite-ammo start weapon, unlike Q3's melee gauntlet). Out-gunned MG/CG and
  *hurt* bots still flee (all Plan 36/37 tests still green). Re-run A/B (110s): **q3 2 kills/1 death
  K/D 2.00 vs main 3/4 K/D 0.75**. Pure-q3 fleet (6 navmodes ×2, 90s): **9 frags, 0 panics, 0
  kicks**; q3-race best (K/D 2.50). q3 connects, navigates multi-level, perceives, fights, frags —
  competitive with main.

## 2026-06-19 — Plan 38: Quake 3 personality roster (`--q3char`/`--q3chars`)
- Turned the single default `q3` brain (Plan 37) into a selectable **roster** of named Q3
  characters. Additive; non-`q3` brains unchanged.
- `q3char::Q3CharPreset` (clap `ValueEnum`: `grunt`/`major`/`sarge`/`camper`) — `character()` →
  the `Q3Character` preset, `tag()` for names/scoreboard, `skin()` for a distinct per-character
  Q2 skin (male/grunt, male/major, male/sarge, female/athena).
- Wiring: `build_brain` gained an `Option<Q3CharPreset>` arg (only the `Quake3` arm reads it;
  `None` → `Q3Character::from_skill(skill)`, the Plan 37 default — every other arm + scenario pass
  `None`). `--q3char` on `connect-one`/`run`, `[fleet].q3char` config (+ `q3char_preset()`),
  threaded through `bot_task`/`bot_supervisor_loop`/`run_single`/`run_fleet`. A selected character
  pins its skin even as a single bot.
- `competition --q3chars grunt,major,…` — a per-character group axis that **only expands the `q3`
  brain** (others get a single `None` sub-group). Group = `(mode, brain, q3char?)`; tag becomes
  `q3-grunt-astar` etc.; disjoint qport blocks per group; each character wears its own skin. Group
  counting (`groups_per_mode`) folds the variable char-count into the maxclients clamp.
- **T3 observed-inventory: DEFERRED.** The Plan 37 blaster-floor already makes held-weapon
  aggression competitive (q3 K/D 2.0 vs main 0.75; roster spread clean), so mining pickups/
  obituaries for a "best-owned" weapon is unnecessary for now — left as a future option (the
  `bot_aggression` doc already flags it). No `observed.rs` change.
- **T4 LIVE TUNING (q2dm1, 160 s, 8 q3 bots): presets validated, no float changes.** Frag spread
  is intentional + balanced: **major 5/1 (K/D 5.00, precise), sarge 5/4 (1.25, aggressive/mobile),
  camper 1/1 (1.00, cautious), grunt 0/7 (0.00, cannon fodder)**. 0 panics. Recorded in
  `mode_perf.md`. The Plan 36 preset value sets stand as-is.

## 2026-06-19 — Plan 40: swim movement, water-exit & navmode ranking
- Goal: make the brain execute the Plan 39 swim edges — dive, swim the tunnel, surface, and
  climb onto the q2dm1 railgun ledge — then prove + rank `spawn-to-weapon railgun` across navmodes.
- New `brain::water`: `water_level(cm, origin) -> 0..3` recomputed like `PM_CategorizePosition`
  (`pmove.c:765`; waterlevel is NOT on the wire) by sampling `CONTENTS_WATER` at feet/mid/eye.
  `is_swimming(level) = level >= 2`. Pure + unit-tested against `world::water_channel_world`.
- RunTesterBrain (the `spawn-to-*` driver) swim path: on a swim edge / `waterlevel>=2`, use the
  RAW 3-D look-ahead (`pursue_target`, not `_safe` — no floor to validate underwater), set
  `intent.up = clamp(dz/32, -1, 1)` (sustained, NEVER `mv.jump()` in water) and pitch toward the
  3-D target. Water-exit climb-out: when the swim edge's target is a dry node above
  (`water_level(target)==0 && dz>0`), force look-up `pitch=-20` (Q2 water-jump gate is `<=-15`,
  `pmove.c:414`) + `up=1` + forward, held `EXIT_HYSTERESIS_TICKS=12` so it clears the lip.
- Recovery SUSPENDED while swimming (skip the whole `evaluate`): `find_best_direction` steers
  away from water and the 4u/1s StuckDetector false-fires on 0.5× swim speed.
- MainBrain got the same swim override (section 8) so LIVE bots swim too; combat aim wins the view
  pitch when firing (else pitch toward the 3-D target). Recovery also gated when swimming.
- Recorder `S` flag (waterlevel>=2) wired from the scenario sample.
- **LIVE PROOF (q2dm1, local yquake2 dedicated, 2026-06-19):** `astar` `reached=true` ~11–27 s;
  the bot's log shows 46/93 frames `S`-flagged with z 238→434 = dive→swim tunnel→surface→exit.
- **Navmode ranking** (see `mode_perf.md`): `astar` + all A*-backed hybrids reach; pure `navmesh`
  fails (no navmesh water — Plan 39 scope). `hybrid-race` reaches because it plans both and the A*
  plan (with the swim route) wins.
