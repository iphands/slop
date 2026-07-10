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

## 2026-06-19 — Plan 42/43: moving-platform (func_train) + lift ride behavior (q2dm3)

- **New seam:** `EdgeKind::Ride` + `RideInfo` (board/far/dismount/model_index/vertical/board_ent/
  far_ent) in `world::navgraph`; `Navigator::current_edge_is_ride()`/`current_ride_info()` mirror
  the swim seam through `NavigationDriver` + all four hybrids. `brain::ride` decides
  approach/wait/cross; both `MainBrain` and `RunTesterBrain` execute it; stuck-recovery suspended
  while `ride_active` (same discipline as `swim_active`, Plan 40).
- **Lifts (func_plat/func_door) now ride.** Tagged as *vertical* ride edges (was a plain Walk
  edge the brain tried to "walk" straight up → stuck at the shaft). Verified live on q2dm3: the
  bot rides the lift up to the upper levels (z≈393) where it previously could not leave z-16.
  This starts Plan 31 (the `lift_penalty`/`ELEVATOR_PENALTY` hack can retire once multi-bot
  de-conflict lands; `generate-map-cache --lift-penalty` now lets you build a lift-preferred cache).
- **Trains (horizontal) are the hard part.** Detection now works (wire-origin match, see
  pitfalls), board ledges anchored to solid ground (no approach-deaths), and a stateful
  board→ride-hold→dismount machine. BUT reliably riding a *moving* train across q2dm3's pit
  still fails — the bot falls into the pit (z-104) at the loop-train crossings (~6-7 deaths/110s).
  The board window is brief (train at a corner ~0.8s every ~8s loop), PVS visibility of the train
  at that instant is unreliable, and "ride-hold while carried" needs the train's live top surface
  which isn't traceable (inline models aren't in the CM). **Next:** track the train's live origin
  to keep the bot centered on the moving top, and time the board to the corner-dwell window.
- **Reachability vs. execution:** q2dm3 railgun (`--instance 1`, `(768,816,208)`) is
  **A\*-reachable from all 7 spawns** (path = walk + jump-bridge drops + 2 train rides); physical
  reach is gated on train-riding above. The **quad** (`item_quad (192,320,216)`) is **not yet
  nav-reachable** — it's in the upper level (comp0) which has no spawn-side up-route in our graph;
  that's the broad q2dm3 floor fragmentation = **Plan 35**.

  **Bonus (Plan 35 spillover):** `bridge_components_via_jump` also restored **q2dm2 + q2dm5** to
  full spawn connectivity (were failing per Plan 34); q2dm1/4/8 unchanged (still full). q2dm3/6/7
  remain partial (deeper fragmentation). So the jump-down floor bridge is a partial Plan 35 fix.

## 2026-06-19 — Plan 43 T7: JUMP on/off movers — railgun REACHED

User feedback: "we need to jump on/off lifts/platforms/trains — I always jump." Decisive fix.
- **Board**: when the train is at the board corner (`board_ent` matched), `mv.jump()` + step on.
- **Carried**: track the train's **live top-center** = `entity.origin + (far - far_ent)` (the
  constant brush-origin↔stand offset) and steer to stay centered — NO jump (it'd launch us off).
- **Dismount**: when the train reaches the far corner (`far_ent` matched), `mv.jump()` + step off.
- **Lift**: hop on at the bottom (`board_horiz > 32`), ride still while rising.
- **Result**: q2dm3 `spawn-to-weapon railgun --instance 1` now **3/4 reach on astar** (32/91/108 s)
  and **3/4 on hybrid-race**; deaths fell ~7→~1. `hybrid-fallback` 1/4 (navmesh has no rides).
  Ranking recorded in `mode_perf.md`. The "hold blindly while carried" version drifted off the
  moving train into the pit; live-center tracking is what made riding reliable.

## 2026-06-19 — Plan 35: ladder nav support → q2dm3 FULLY connected (7/7)

User insight: q2dm3 reaches the upper level via **ladders** ("elevator OR ladder"). q2dm3 has
**8 `CONTENTS_LADDER` brushes** our nav ignored, leaving the upper bulk (comp0, z152-600) cut
off from the spawn floors (only 3/7 spawns connected).
- **Nav**: `add_ladder_edges` (`build.rs`) parses CONTENTS_LADDER brush AABBs, anchors a floor
  node at the ladder base + top (`nearest_ground`, now 3-D so the top snaps to the top ledge),
  and adds a vertical **ladder** ride edge (`RideInfo.ladder`, cache v17). 3 ladders wired →
  **q2dm3 now 7/7 spawns connected** (`generate-map-cache q2dm3` clean, no `--allow-failures`).
  Bonus: q2dm7 3/6→4/6; q2dm1/2/4/5/8 stay full; no regression. Railgun still 2/2.
- **Brain climb**: on a ladder ride edge, face the ladder center (`board_ent`) so the 1u forward
  trace hits CONTENTS_LADDER (`pml.ladder`), press forward + `up=1.0` (Q2 `PM_AddCurrents`:
  `upmove>0` → climb), step off near the top. **Partially working** — the bot climbs z-16→~z120
  but **stalls ~40u below the ladder top** (loses ladder contact / drifts off the narrow face).
  Needs face-centering on the ladder while climbing (next tuning step), like the train
  live-center tracking that fixed the train rides.
- **Quad** still nav-isolated: its ledge is walled except a too-tall (56-72u) jump-up from the
  mid-floor and a blocked drop from directly above (a solid pillar z240-408). Its real entry is a
  specific platform-jump from the upper level — a remaining reverse-engineering task.

## 2026-06-19 — Plan 35 cont.: q2dm3 QUAD nav-reachable via *10 over the lava

User correction: the quad is reached by **riding the central moving platform (`*10`) over the
lava**, not the ladder (ladders are a separate, kept mechanism). Fixes:
- **func_train two-height ride search** (`try_add_train`): q2dm3's trains use different
  corner→ride-surface conventions — loop trains `*3/*4` ride at the brush **top**
  (`corner.z+size.z`, rising from the lava), but the central `*10` (origin-brushed) rides at the
  **corner level** (`corner.z`, a stem hangs below). We try BOTH heights and keep whichever
  finds adjacent reachable ground. → `*10` now connects its boarding ledge to the **quad ledge**;
  **the quad is A\*-reachable from all 7 spawns** (path: 33 walk + 8 jump + 2 ride).
- **Directional ladder climb**: ladder edges are bidirectional; the brain now drives
  `up = (dismount.z - pos.z).signum()` (climb up OR down) and steps off within 20u of the exit
  level — fixed a 463-frame stall where a *descending* path pressed `up` against the ladder.
- **3-D `nearest_ground`** so a ladder top snaps to the top-floor ledge (was topping out ~40u low).

**Physical reach still 0/4** for the quad: the full route is long (down-ladder DESCENT + the
`*10` over-lava ride + 8 jump-bridges) and two execution gaps remain — (1) **ladder descent
initiation**: the descent board node sits ~45u from the shaft on a separate ledge, so the bot
jams at the top instead of dropping in (node should anchor AT the shaft mouth); (2) the **`*10`
over-lava ride**: the bot falls to z12 (into the lava) instead of riding to the quad — the board
ledge / over-lava ride timing for this specific oscillator needs the same care the railgun loop
trains got. NAV is correct (7/7 + quad reachable); these are execution-tuning follow-ups.

## 2026-06-19 — Plan 35 cont.: q2dm3 quad *10 over-lava ride (in progress)

- **Single-platform constraint (user):** only `*10` reaches the quad and it's too small for 2
  bots — the quad scenario must run **`--count 1`** (multi-bot wait/de-conflict is Plan 31).
- Quad route (spawn 0): up-ladder `(-625,679,-15)→(-553,679,232)` → walk → **ride `*10`**
  `(191,-329,216)→quad (191,199,224)` over the central lava. NAV is correct (7/7 + quad
  A*-reachable). Physical ride bugs fixed this pass:
  1. **Board onto the platform-top, not the far dismount** — aiming at the quad (528u north)
     launched the bot off the ledge past the platform into the lava.
  2. **`stand_offset`** (wire-origin→platform-top) stored in RideInfo so `train_stand_now`
     tracks the *actual* moving top (the two-height refactor had broken its offset).
  3. **Step on when level; jump only to hop UP** — a full 1s jump arc lets the 60u/s platform
     slide out from under the bot.
  4. **Stand + let Q2 push carry** while centered (chasing the center at sprint speed runs the
     bot off the leading edge).
  - Result: the bot now boards and **rides `*10` at z≈200 for ~14s**, but full completion is
    still flaky — occasional ladder-climb stall (~z120) before `*10`, and the final dismount at
    t2 onto the quad ledge. Quad physical reach still 0; needs more single-bot ride tuning.

## 2026-06-19 — Plan 35/43 FINAL state: railgun rides; quad *10 ride is a control wall

After extensive iteration on q2dm3's movers:
- **Railgun (`spawn-to-weapon railgun --instance 1`) WORKS**: astar reaches **1–4/4** (high
  spawn-variance; 4/4 and 3/4 observed). Bot rides the `*3/*4` loop trains + the `*2` func_plat
  lift. This is the user's first target — done.
- **q2dm3 fully spawn-connected (7/7)** via ladder nav edges (ascent + descent both work) +
  train ride edges + jump-down floor bridges. `generate-map-cache q2dm3` clean.
- **Quad (`spawn-to-item quaddamage --count 1`): nav-reachable via `*10` over the lava, and the
  bot DOES board `*10`** — but the ride doesn't sustain: it falls off after ~1.5 s. Root cause is
  a genuine control-feasibility wall, not a missing feature:
  - `*10` is a SMALL platform OSCILLATING a long way (t1 y−296 ↔ t2 y+88 = 384u) at 60 u/s over
    **instant-death lava**, boarded across a ~33u lava gap and dismounted across another gap onto
    the quad ledge. Q2 jumps overshoot the ~80u platform (~290u arc); a slow approach falls in
    the gap; once airborne there's no air-braking; and staying centered for the full 6 s ride is
    brutal at 10 Hz external control. The railgun's `*3/*4` loop trains work because their rides
    are SHORT and tight; `*10`'s long over-lava oscillation is the hard case.
  - Tried (all committed/iterated): jump-on/lead/low-speed/full-speed boards, stand+push,
    world-frame and platform-relative momentum braking, near-edge targeting, grounded on-top
    commit. Each helped a phase but none made the full board→ride→dismount reliable.
  - **Closest approach ~229–236u** (the quad ledge vicinity, but at z≈12–17 after falling).
- **Single-platform constraint**: the quad scenario must run `--count 1` (one small platform;
  multi-bot wait/de-conflict is Plan 31).

## 2026-06-19 — QUAD REACHED (q2dm3 *10 ride) — measure-first debugging

`spawn-to-item quaddamage` now REACHES the quad (closest=8, ~20-26s) when the bot starts near the
board. The whole 60-iteration stall was caused by GUESSING the platform physics; one live capture
(`QBOTS_OBSERVE_MOVERS=1 connect-one`) + ride telemetry exposed the real bugs in minutes:
- `*10` deck is z216 (corner-level), NOT the bbox top z410 (it's a deck with tall rails).
- Brush models stream with **modelindex=0** → match by position; and there are many **null
  `[0,0,0]` world entities** that made `platform_present` fire constantly (false `train_here`).
- Q2 trains PUSH riders → carry = **zero input** ("sit still", per the human who plays the map).
- The nav advances off the ride edge mid-transit → must LOCK the ride active while boarded.
Ride sequence that works: wait at the board ledge → when the platform is at the near (far-from-quad)
corner, HOP onto its live deck → commit only when GROUNDED on the deck → zero-input carry (locked)
→ when the platform nears the far corner, JUMP off onto the quad ledge.

**Remaining limiter (separate problem):** the ROUTE to the board from FAR spawns (8 jump-bridges +
a ladder through the fragmented q2dm3 upper level) is unreliable, so `--count 4` (bots spread across
far spawns) reaches 0-1/4 even though the ride itself is solid. From the near spawn (spawn3, on the
board's ledge) it reaches reliably. Improving far-spawn route reliability is the next task.

## 2026-06-19 — far-spawn route to the quad board: blocked by bogus upper-level bridge edges
Improved the ladder ASCENT (face the EXIT/dismount, not the ladder center, so the bot climbs
up-and-over onto the top ledge instead of topping out on the entry side and falling; hop near the
top). This let a far bot climb + get within 107u (was trapped on an isolated z152 island before).
BUT far-spawn → board is still ~0/6: the A* route over the q2dm3 upper level uses **over-long
"walk" bridge edges that are hull-BLOCKED** — e.g. (-121,-161,216)→board(191,-329,216) is a 354u
"Walk" whose hull trace stops at fraction 0.07 (point trace clear). The bot can't physically
traverse it. Root cause = nav-graph quality on the fragmented upper level (bridge/seed edges not
hull-validated for the player hull over distance). Fixing it = a substantial Plan 35 nav-generation
task (hull-validate + split long bridges; resample the upper level). The RIDE itself is solved and
the quad is reached from the near spawn (spawn3, on the board ledge) — the natural human start.

## 2026-07-09 — Plan 43 T6 closeout: six-navmode q2dm3 ride ranking
Completed the q2dm3 ride ranking (mode_perf.md) with the three previously-untested navmodes,
live on noir.lan:27910 (cache spacing 24, --lift-penalty 0, --max-secs 150).
- **Railgun (`--instance 1 --count 4`):** astar 3/4, hybrid-race 3/4, **hybrid-hier 3/4** (times
  37/55/91 s), hybrid-fallback 1/4, navmesh 0/4, hybrid-segment 0/4.
- **Surprise:** `hybrid-hier` RIDES (predicted 0). Its A* *local* planner inside the navmesh
  corridor carries the ride edges — so ride traversal works on **every A*-backed navmode**, not
  just pure astar. Only the pure-navmesh backend (navmesh; hybrid-segment's open segments) lacks
  ride edges — the same structural gap as water (Plan 40, deferred navmesh-water follow-up).
- **Quad (`--count 1`, random spawn):** 0/1 on all three (as with astar) — reaches only from the
  board-adjacent spawn3; far-spawn routes remain Plan 35. Not a ride regression.
- **T4 recorder `P` flag** shipped (`35cd30643`): `riding` frame field + `P` char, set from
  `current_edge_is_ride()` in the scenario sampler (phantom-target moved `P`→`T` to keep the
  traversal trio S/P/L contiguous — Plan 46 adds `L`).
Plan 43 is now 100% and moved to completed/.

## 2026-07-09 — Plan 46: shared TraversalExecutor (ladder/swim/ride parity for ALL brains)
Extracted the three drifted per-brain traversal copies into one `brain::traverse::TraversalExecutor`
(gates() → swim/ride suspend-recovery gates; apply() → movement override + S/P/L flag). Every brain
delegates now:
- **runtester** (T2): verbatim adopt (the byte-preservation anchor). Live q2dm3 railgun ride
  (astar --count 4) = **4/4** (≥ the 3/4 baseline). Deleted ~180 lines of inline swim/ride/ladder.
- **main** (T3): GAINED ladders + the stateful board/carry lock (previously only a *stateless* ride
  + partial swim). Live q2dm3 railgun `--brain main` reaches (34.8s); recorder shows 77 `P` ride
  frames and **ZERO `P`+`R` frames** — recovery correctly suspended during traversal.
- **q3** (T4): GAINED ALL traversal (previously none — a q3 bot couldn't swim/ride/climb in a
  match). Added inside `locomote` (path-following stage; combat_drive with a visible enemy stays
  non-traversing = accepted v1 priority). Live q2dm3 railgun `--brain q3` reaches (1/3, rides the
  `*3/*4` trains + `*2` lift) — was structurally 0/N before.
Key design calls:
- Executor OWNS movement + view while a traversal edge is active; the brain keeps only the fire
  decision (attack button) — the bot fires along the traversal heading (accepted for v1). Movement
  is view-relative, so the view can't be split from movement.
- `apply()` takes `Option<&CollisionModel>` (main/q3 hold cm as Option) — water samples degrade to
  0/dry via `cm.map_or(0, …)`, matching the brains' own fallback.
- The best copy of each machine is **runtester's** (not main's, despite the plan's parenthetical) —
  it's the self-contained regression anchor; main's swim was partial (vertical-only) and its ride
  stateless. Lifting runtester's preserved its behavior and *upgraded* main/q3.
- New recorder `L` flag (ladder) split from `P` (platform), derived in the scenario sampler from
  `nav.current_ride_info().ladder` (consistent with how `P` is derived from nav state, T4).
**q2dm1 swim matrix (2026-07-09, closed):** `spawn-to-weapon railgun --count 3 astar` =
runtester 3/3, main 3/3, **q3 3/3** (q3 had zero swim before). All brains swim via the shared
executor. Plan 46 100% done, moved to completed/.

## 2026-07-10 — Plan 30 (resource decisions) closeout + a variance lesson
Shipped: static map-item table (`BrainMap.items`, `classify_item_classname`+`build_map_items`,
52/83 spawns on q2dm3/q2dm1), PVS-honest `ItemMemory` (respawn timers), **bounded** Flee→known-health
seek (≤900u A*), and ammo-aware `select_best_weapon` (dry held weapon → Blaster fallback). q3 untouched.
**Reverted:** the roam-item patrol (main constantly path-seeking items) — a clear regression on q2dm3
(main kd 0.12 vs 0.19 without; bots stopped fighting to item-hunt across the lava).

**Variance lesson (important):** single 5-min / 6-bot competition samples are TOO NOISY for combat
A/B. q2dm1 head-to-head, q3 as control with **identical code**, swung q3 kd **1.00 → 0.86 → 2.60**
across three runs — far larger than any main-side Plan 30 effect (main 0.69/0.50/0.28). So the
"Plan 30 regressed main 0.69→0.50" read was itself noise. **Do not tune combat on one-off
competitions.** Real behavior verification needs the Plan 47 multi-run acceptance harness
(many runs / longer / more bots, averaged). Plan 30's behavior changes are kept because they are
principled + north-star-aligned + conservatively bounded, NOT because a single run "proved" them.

## 2026-07-10 — Plan 28 (weapon matchups): own-weapon range bands shipped; enemy read blocked
- **Enemy-weapon inference is not possible on this server.** VWep verification (QBOTS_P28_DEBUG):
  every player entity carries `modelindex2 = 255` (sentinel, empty CS slot) — the enemy's held
  weapon is NOT on the wire (pitfalls.md). So T1's `from_wield_model`/`held_weapon` and T3's
  `matchup_score` engage-bias ship **dormant** (correct + unit-tested; light up on a VWep-per-weapon
  server). We did NOT wire a dormant enemy-weapon gate into main's hot path.
- **T2 shipped active — per-weapon ideal-range positioning** (`weapons::ideal_range`→`RangeBand`)
  replaces main's fixed `IDEAL_DIST=160`/`BACKUP_DIST=80`: shotguns hug (32/128), rail holds out
  (300/700), splash stays outside its own min_safe (RL 160/500, BFG 560/900). Uses only OUR weapon
  (known via gunindex). This is the enemy-independent, always-available half of "fight at the right
  range for your weapon".
- **Verification:** by mechanism (unit tests: band character, splash-safety, matchup ordering) +
  no-regression (3-min q2dm1 comp: bots play, 0 panics/kicks). A clean combat-kd A/B is impractical
  here: the N=5 noise floor (~0.6 K/D) dwarfs a positioning tweak, and q2dm1 6-bot short runs barely
  generate encounters (this sanity run: 1 kill in 3 min). Resolving small tactical effects needs a
  higher-encounter setup (more bots / longer runs / a tighter map) — a Plan 47 harness extension.

## 2026-07-10 — Plan 27 (persona parameters): main gains a real personality
Shipped `brain::persona::Persona` — [0,1] traits (aggression/risk_tolerance/weapon_pref/camper/
chase_commit/item_greed) + tactical getters (flee_health/kite_health/kite_dist/roam_dwell). The
LOAD-BEARING contract (Risk #2): `Persona::default()` reproduces main's pre-Plan-27 constants
EXACTLY (30/50/450/50), unit-tested — so converting main's global consts to `self.persona.*()` is
behavior-preserving for every current bot. 4 named presets (rusher fights hurt + closes; sniper
holds range + bails early; scavenger hoards; guard camps) selectable via `connect-one --persona`.
`chase_commit`/`item_greed` traits are placed for Plans 29/30 to consume. Competition-roster
selection + a live spread table are a follow-on: they're a demo, and per the harness lesson a kd
spread across 4 personas is noise-limited at feasible sample sizes — the persona *values* are the
tested contract, not a single roster run.

## 2026-07-10 — Plan 29 (engagement): winning/losing read + break-off
Enemy health/weapon aren't on the wire, so "am I winning?" is inferred from OUR state via
`brain::engage::EngageTracker`: pressure (fire-on-target proxy, accumulates while firing with LOS)
+ losing (sustained incoming damage with low pressure). MainBrain updates it each combat tick and,
in the no-LOS chase branch, BREAKS OFF (→ retreat_goal) when:
- **third-partied** — took damage while the target is out of LOS (an unseen shooter is on us), or
- **losing** AND persona `chase_commit` < 0.7 (a dogged rusher keeps chasing; a sniper bails).
This is the "pick and finish fights" disengage half. The velocity-extrapolated Hunt goal (pursue
*through* the doorway) is deferred — it needs the FSM `Hunt` state to carry the enemy's last
velocity (state surgery); the existing Hunt-to-last-pos + this break-off gate cover the core.
Verification: `EngageTracker` unit tests + no-regression sanity (q2dm1 comp: main engages, 3 kills,
0 panics; kd within the established noise band). A clean kd A/B is noise-limited (harness lesson).

## 2026-07-10 — Plan 33 (heatmap preference pull-up): danger weighting tracks mood
main's `heatmap_weights()` now scales the skill-derived base by `persona::HeatmapMood` (health +
FSM engaged/hunting, from the previous tick) × persona `risk_tolerance`. Hurt → danger weight up,
crowd-seeking down (route around kill-zones); hunting/engaged → danger down, crowd up (cut through
to the kill). Calibrated so NEUTRAL mood + default persona = (1.0, 1.0) → base unchanged
(byte-preserving at full health/idle, unit-tested). q3/sentry/runtester use the (0,0) default,
untouched. Deterministic proof: extended the Plan 08 pipeline test — same bot, same 1-death
kill-zone, hurt detours via D, healthy-hunter cuts through B. No live run needed (the mechanism is
graph-deterministic, dodging the combat-noise problem).

## 2026-07-10 — Plan 32 (underwater breath): bots breathe before they drown
No air model existed — a loitering bot would sit at waterlevel 3 past Q2's 12s and eat escalating
drown damage (view.c:763,863). Shipped:
- `water::AirClock` — client-side mirror of the server's air clock (we compute waterlevel
  ourselves, so counting continuous level-3 time tracks the server to a tick). 12s budget, 2s
  safety margin; `must_surface(time_to_surface)`; `on_unexplained_damage` re-syncs when observed
  drown damage says the server's clock ran ahead (main calls it on damage-while-swimming-no-enemy).
- **Surface-seek override** in the shared TraversalExecutor: `gates()` (now takes `dt`) ticks the
  clock every frame; when air is critical `apply_swim` abandons the swim path and drives straight
  up (full up-thrust, −70° pitch) until one breathable frame resets the clock. Priority above
  normal swim steering; `time_to_surface` = upward contents-scan / SWIM_UP_SPEED (60 u/s, pinned
  from Plan 40's measured dive logs, conservative).
- **Verification:** end-to-end unit test (submerged past budget with a DOWNWARD target → full-up;
  one breath → the dive resumes) + live q2dm1 railgun swim regression **3/3, zero damage** (no
  false surfacing on in-budget routes). The live forced-loiter can't run through the harness — the
  scenario preflight rejects unreachable goals by design (a good guard, discovered here).
- Deferred: T3 dive gating (needs Navigator path introspection; over-budget dives now self-correct
  by bobbing up for air — which is also what a human does). All brains get this via the executor.

## 2026-07-10 — Plan 47 closed: the acceptance suite caught its first real bug
Completed T1 (EVT counters) + T4 (showcase) and closed the capstone. The headline: the counters'
FIRST run exposed **weapon-switch thrash** — 4179 `use` requests in one 5-min match (~14/s,
Blaster↔Railgun). Cause: the Plan 30 dry-gate read `STAT_AMMO` (the ammo of the weapon the WIRE
holds, via gunindex) but gated the *optimistic* held model in CombatDriver — on disagreement it
flipped every tick, and each request reset the 0.2s fire lockout so a thrashing bot BARELY FIRED.
This was live in every run since P30-T4, including the N=5 main baseline (main 0.36 is therefore
suspect — re-baseline at N≥5). Fix: re-sync the optimistic model from gunindex each tick + a 1s
switch-request cooldown. Post-fix showcase: switches 4179→493, total kills 16→24, and main matched
q3 K/D (0.26 vs 0.25) for the first time (single-run caveat).
Showcase behavior counters (5-min main-vs-q3, q2dm3): 44 traversal legs mid-combat (35 ladders,
9 rides), 89 chase conversions, 10 third-party breaks, 0 drownings, 0 panics. The north-star
"human-like play" is now a measured, repeatable run: `acceptance matrix` (traversal gates) +
`acceptance --control` (multi-run K/D) + EVT greps (behavior).
LESSON (again): instrument first — the counters found in one run what weeks of scoreboard-watching
missed. Optimistic client-side models MUST re-sync from the wire when it speaks.
