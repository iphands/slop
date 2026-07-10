# Plan Series — Dependency Chain

Master ordering of all qbots plans. **Update this file whenever a plan is added,
starts, or completes.** Status values: `pending` | `in-progress` | `done` | `blocked` | `skipped`.

> **North star (user directive, 2026-07-09):** human-like bots that successfully navigate
> whole maps (ladders, swimming, platform/lift riding), collect items and weapons, and play
> naturally — chasing for the kill, collecting health when hurt, switching weapons for
> close/far combat.
>
> **Recommended implementation order for the active set:**
> `42(T6)` → `43(T4,T6)` → **`46`** → `35` → `30` → `28` → `29` → `27` → `31` → `32` → `33` → `44` → `47`.
> (46 before 35's live re-checks so all brains can exercise the fixed routes; 27 can land
> any time before 33; 44 is diversity, not core; 47 closes the series.)

| Plan | Title | Depends on | Status | Milestone / Notes |
|------|-------|-----------|--------|-------------------|
| **01** | Workspace scaffold | — | done | `crates/` skeleton, `.gitignore`, `justfile`, build gates |
| **02** | Wire codec (`q2proto`) | 01 | done | `MSG_*` R/W, `usercmd` delta, InfoString, OOB framing |
| **03** | Connection (`client`) | 02 | done | handshake + netchan + spawn → **bot connects & enters the game** (verified live) |
| **04** | Frame loop & movement | 03 | done | decode frames + real `Usercmd`s → **bot walks** (verified live; T5 pmove deferred) |
| **05** | World model (`world`) | 04 | done | `.bsp` parse → trace + PVS + nav graph (T1-T4 verified; T5 deferred) |
| **06** | Brain (`brain`) | 05 | done | perceive → navigate → fight → **single bot scores frags** (verified live 2026-06-15) |
| **07** | Eraser-derived brain enhancements | 06 | done | port Eraser's combat aim/lead/jitter, weapon-select, projectile danger dodge, skill/personality (`distilled/eraser.md`) → **bots fight like Eraser** (verified: 6 frags/30s, 3-bot fleet). RL-retreat deferred (enemy weapon not on wire). |
| **08** | Danger/popularity heatmap nav | 05, 06 | done | runtime risk overlay on the static nav graph (novel — Eraser can't) → **route around death-traps, toward hot lanes** (deterministic integration test verifies detour→decay-restore + skill-scaling; live exercise pending — the observed 8-bot fleet ran pre-heatmap code) |
| **09** | Fleet (`qbots` bin) | 06 | done | roster config, supervisor (shared nav cache, stagger, reconnect/backoff, graceful shutdown), per-bot logging, rate-safe pacing, `run`/`connect-one`/`status` CLI → **8-bot fleet verified live** (qb0-qb7 connected, frags accumulating, no kicks; `qbots status` OOB lens) |
| **10** | Movement test harness | 09 | done | BSP **entity-lump** parse (spawn points + `weapon_*` origins), `spawn-to-spawn` / `spawn-to-weapon` CLI, per-frame `MovementRecorder` → structured `./logs/<scenario>/<ts>.<bot>.log` → **measure pathing accuracy & elapsed time** (the lens for 11-14). Verified live 2026-06-15: both baselines fail-to-reach (mean_speed 33/11 u/s; 196/239 hindered frames) — the contract 11–14 must beat. |
| **11** | Honest LOS perception | 10 | done | BSP-trace LOS gate on enemy selection, FSM transition, fire/chase, nav-to-enemy (`has_los_player`); 2-frame sight grace; `phantom_target` recorder flag; stale `fire_allowed=false`. Bots no longer walk into walls at walled enemies. |
| **12** | Steering controller | 11 | done | turn-rate limiting, look-ahead/"pursue" target + arrive, anti-orbit Z-aware node-advance, facing-validated forward, aim/move-yaw separation (circle-strafe) → **bots move like humans, not orbit/spin/wedge** |
| **13** | Stuck recovery & wall avoidance | 12 | done | unified 4u/1s `StuckDetector`, 6-dir fan-out `find_best_direction`, `Recovery::evaluate→RecoveryAction`; `R` recorder flag; old blind-reverse + dual detectors removed → **bots unstick reactively** |
| **14** | Nav-graph & path quality | 10 | done | funnel/string-pull smoothing (smooth_path + smooth_with_cm), spawn seeding (seed_spawns), jump-down links (detect_jump_edges + EdgeKind), path_efficiency recorder metric → **shorter, smoother, faster routes** (code complete; live elapsed-time verification pending) |
| **15** | Scenario nav parity | 11–14 | done | scenario.rs was missing seed_spawns, detect_jump_edges, smooth_with_cm, jump-edge action, and Recovery → **spawn-to-spawn reached=1** (tracker was stale; verified done via git log 2026-06-16 — see `completed/15_scenario_nav_parity_tracker.md`). Further ad-hoc fixes (component bridging, `--count`) landed outside any tracked plan; folded into Plan 19. |
| **16** | *(prefix retired — no single Plan 16)* | — | n/a | Several untracked exploratory docs used the `16_` prefix ad hoc: `16_bsp_parsing_fix(+_summary)` — done, the one confirmed bug (model mins/maxs margin) shipped in `b72600ae2`, moved to `completed/`; `16_bsp_nav_metrics`, `16_observation_learning` — abandoned (stale baselines / outdated premise), moved to `abandoned/`. Numbering resumes cleanly at 17. |
| **17** | BSP/collision hardening & STEP fix | 05 | done | Fresh vendor re-audit (2026-06-16) confirmed the shipped model-margin fix is correct and found one new bug: nav graph `STEP=24` vs real `STEPSIZE=18` (`pmove.c:32`) → bots can't actually climb height diffs the graph calls walkable. Also: entity-tokenizer `//` comment parity, vendor-constant pin tests, backfill `pitfalls.md`. Moved to `completed/`. |
| **18** | Ahead-of-time map cache (`generate-map-cache`) | 17, 09 | done | `generate-map-cache --map 'q2dm*' --jobs 4` caches all 8 maps in 9.5s (vs 22.9s --jobs 1). `cached_map_nav()` in `world/build.rs` skips generation on cache hit; both supervisor and scenario wired. `/data/mapcache/` gitignored. |
| **19** | Nav graph quality & 8-bot fleet reach validation | 17, 18, 15 | done | Closes the user-facing reach goal: `spawn-to-spawn`/`spawn-to-weapon` with `--count`/`--max-secs` + per-bot pass/fail summary, scenario goal seeding. **Closed 2026-06-19** — `hybrid-race` navmode reaches reliably enough that the dedicated 8/8 validation push is unnecessary for now; further reach-quality work can reopen as a follow-up plan if needed. |
| **21** | Competition runner | 09, 20 | done | `qbots competition` spawns N bots per nav `--mode` in one process (shared `NavCache`), one distinct skin per mode, and prints a per-mode frag scoreboard (`FleetStats` grouped by name prefix). In-process (no 6× nav rebuild); makes `mode` per-bot in the fleet supervisor. |
| **22** | Brain seam extraction | 06, 07, 12, 13 | done | Extracted the dissolved decision/steering body of `bot_task` into a single `brain::Brain` (owns combat/fsm/danger/steering/recovery/skill/roam; `Navigator` injected per tick). `bot_task` is now thin orchestration (−~280 net lines in `main.rs`). Verbatim lift; validated live via `connect-one` (full combat+nav+FSM pipeline through `brain.tick`). Nav + driver untouched. **T4 (migrate `scenario.rs` onto `Brain`) closed by Plan 26** — the scenario tick became `RunTesterBrain` and the inline duplication is deleted. |
| **23** | Brain plugin **core** (`trait Brain`) | 22 | done | Turn the single concrete `Brain` into a plugin contract: `trait Brain` + `BrainContext`/`BrainOutput`/`BrainConfig`/`BrainMap` + `BrainKind` enum + `build_brain` factory (mirrors `NavMode`/`build_navigator`) in `brain::brains::core`; existing brain implements it behind `Box<dyn Brain>`. Establishes `context/brain_notes.md` (append-on-every-brain-plan rule). **Behavior-preserving.** Supersedes the old single "Behavior/persona" Plan 23, which is now expanded into 23–32. |
| **24** | `main` brain plugin | 23 | done | Relocate the concrete decision body into `brains/main::MainBrain`; add a minimal `SentryBrain` reference plugin (proves the seam runs with >1 brain); `build_brain` dispatches both. `main` behavior byte-identical. (Scenario migration / Plan 22 T4 moved to Plan 26.) |
| **25** | Multibrain selection + `--navmode` rename | 24 | done | `--brain <kind>` on connect-one/run/spawn-*/competition + per-bot `[fleet].brain` config; **any brain × any navmode** (orthogonal axes); rename `--mode`→`--navmode`/`--modes`→`--navmodes` (CLI/help/README/`mode_perf.md`, keep `NavMode` type); `competition --brains` matrix. |
| **26** | `runtester` scenario brain | 25 | done | Lift `scenario.rs`'s non-combat pathfinding tick (verbatim) into `RunTesterBrain` (`BrainKind::RunTester`); `spawn-to-*` drive a selectable `Box<dyn Brain>` (default `runtester`, `main` for A/B); `goal_override` moves to `BrainContext`; delete inline duplication — **closes Plan 22 T4 + retires Plan 15**. CI determinism tests + a live **6-navmode** acceptance sweep (q2dm1, 2026-06-18) both **PASSED** — every navmode reproduces the `mode_perf.md` baseline pattern, zero panics. |
| **27** | Persona parameters (behavior) | 25 | done | **Closed 2026-07-10.** `brain::persona::Persona` ([0,1] traits + 4 presets rusher/sniper/scavenger/guard); `main`'s `FLEE_HEALTH`/`KITE_HEALTH`/`KITE_DIST`/dwell consts → `self.persona.*()`; **default reproduces them EXACTLY** (unit-tested Risk-#2 contract). `build_brain` persona param + `connect-one --persona`. Competition `--personas`/fleet-config selection + live roster = mechanical follow-on (noise-limited; 29/33 consume traits, not selection). |
| **28** | Tactical weapon-matchup reads (behavior) | 27 | done | **Closed 2026-07-10.** T2 shipped active: per-weapon `ideal_range`→`RangeBand` positioning (shotgun hug / rail hold-out / splash outside min_safe) replaces fixed `IDEAL_DIST`/`BACKUP_DIST`. T1 enemy inference (`from_wield_model`) + T3 `matchup_score` ship **dormant** — this yquake2 server sends `modelindex2=255` (no per-weapon VWep), so the enemy weapon isn't on the wire (`pitfalls.md`). Verified by unit tests + no-regression; clean kd A/B impractical (sub-noise + low encounter rate). |
| **29** | Engagement: chase / disengage / third-party (behavior) | 28, 27 | done | **Closed 2026-07-10.** `brain::engage::EngageTracker` (own-state winning/losing: pressure + damage trend, since enemy health isn't on the wire; 4 unit tests). `main` breaks off a chase when **losing** (persona `chase_commit`-scaled) or **third-partied** (damage while target out of LOS). Vel-extrapolated Hunt pursuit deferred (FSM state surgery). q3 untouched. No-regression sanity passed; kd noise-limited. |
| **30** | Resource decisions: health & ammo (behavior) | 24, 41 | done | **Closed 2026-07-10.** Shipped: `BrainMap.items` static BSP item table (`classify_item_classname`+`build_map_items`), PVS-honest `ItemMemory` (respawn timers), **bounded** hurt→nearest-reachable-health seek (≤900u A*), ammo-aware `select_best_weapon` (dry→Blaster). q3 untouched. **Reverted** the roam-item patrol (regression). Live A/B **inconclusive (variance** — q3 control kd 1.00→2.60 same code); combat tuning → Plan 47 harness. |
| **31** | Elevator / plat multi-bot de-conflict + remove lift penalty (behavior) | 43, 46 | pending | **Plan file authored 2026-07-09.** Wait-clear outside the shaft, prompt step-off, back-off/retry when another bot holds the lift → **delete `ELEVATOR_PENALTY`/`--lift-penalty` everywhere** (VERSION bump) and close `context/elevator_todo.md`. 10-min 8-bot no-deadlock soak is the gate. |
| **32** | Underwater & breath (behavior) | 40, 46 | pending | **Plan file authored 2026-07-09.** No air model exists — add a client-side `AirClock` (Q2's 12s rule, computed from our own `water_level`), surface-seek override in the traversal executor, air-budget dive gating, post-drown heal hunger. Zero drownings. |
| **33** | Heatmap preference pull-up (behavior) | 08, 27 | pending | **Plan file authored 2026-07-09.** `heatmap_weights` (already brain-owned, `skill.rs:194`) becomes a live persona×mood function (hurt → avoid hot lanes; healthy hunter → seek them); neutral mood byte-preserves today; deterministic detour-by-mood test. |
| **34** | q2dm3 nav: diagnostics + resilient cache batch | 17, 18 | done | Tooling + unblock: `navinspect QBOTS_LIVE=1` (inspect a map that fails the gate), compgaps flat-gap walkability fix, q2dm3 fragmentation diagnosis, and `generate-map-cache --allow-failures` (caches good maps, names failures). Diagnosis found the regression is **broad (5/8 stock maps fail)**, not a q2dm3 quirk → deep nav fix split to Plan 35. |
| **35** | q2dm nav connectivity: hull-valid routes + residual gaps | 34, 42, 43 | in-progress (narrowed) | **Narrowed 2026-07-09:** live verification found the far-spawn-quad limiter is **MIXED** (upper-level route/steering hindering + `*10` ride dismount), NOT a clean nav bug; the naive hull-fix (steps by max(dz,hd)) over-rejects legit step-over edges → broad disconnection. **T1/T2 far-spawn-quad DEFERRED.** Remaining clean scope = **T3** (q2dm6 7/8→8/8, q2dm7 4/6→≥5/6). Quad already reaches from the near spawn (Plan 43); ~1–2/4 from far. Prior connectors shipped (ladders/rides/jump-bridges; q2dm3 7/7; q2dm1/2/4/5/8 full). Kept: `navinspect` PATH edge-kind diagnostic. |
| **39** | Water nav graph (swim nodes + swim edges) | 05, 17, 18 | done | A* `NavGraph` now traverses water: `EdgeKind::Swim`, submerged/surface node sampling (`water_waypoints_multi`), 3-D swim↔swim + dry↔water entry/exit edges (`try_swim_edge`), prune-protection, cache v13, `navinspect contents/watermap/gpath`. **Closed 2026-06-19** — q2dm1 railgun `(240,-384,464)` joins the main component; `gpath` from both baseline spawns returns a path (8 swim edges). Synthetic + gated q2dm1 reachability tests green. Navmesh water deferred (a follow-up). |
| **40** | Swim movement, water-exit & navmode ranking | 39, 10, 12, 13, 26 | done | Brain executes swim edges: `brain::water::water_level` (recomputed from `cm`+origin), sustained `intent.up`+pitch thrust, Q2 water-jump climb-out, recovery suspended while swimming, recorder `S`. **Closed 2026-06-19** — live q2dm1: `spawn-to-weapon railgun` `reached=true` on `astar` (~11–27 s; 46/93 frames `S`, z 238→434 = dive→surface). Navmode ranking: **4/6 reach** (astar + all A*-backed hybrids); pure-`navmesh` + `hybrid-segment` don't (no navmesh water). See `context/mode_perf.md`. |
| **41** | `spawn-to-item` + item/weapon target resolution | 10, 26 | done | `spawn-to-item <item>` + friendly aliases (`quaddamage`→`item_quad`) + `--instance N` (q2dm3 has **two** `weapon_railgun`). Verified: quad→`(192,320,216)`, railgun-1→`(768,816,208)`. Moved to `completed/`. |
| **42** | Moving-platform (`func_train`) + lift nav integration (q2dm3) | 17, 18, 39, 34 | done | `EdgeKind::Ride`+`RideInfo`, `func_train` ride edges (ground-anchored boards), lifts→**vertical ride edges**, `bridge_components_via_jump`, cache **v18** (two-height ride search `13b08c4ae` + `stand_offset` `4cfdd2cb8`). **Railgun AND quad both A\*-reachable from all 7 q2dm3 spawns.** **Closed 2026-07-09** — T6 offline test `crates/world/tests/ride_q2dm3.rs` (asserts path + `Ride` edge) passes vs vendor/baseq2. |
| **43** | Moving-platform & lift **ride behavior** + q2dm3 reach proof / navmode ranking | 42, 41, 40, 26 | done | Ride behavior **SOLVED**: approach/wait/board/carry/dismount with JUMP on/off, wire-origin train detection, live top-center tracking, `active_ride` edge lock, zero-input carry. **q2dm3 railgun REACHED on every A\*-backed navmode** (astar/hybrid-race/hybrid-hier 3/4); QUAD from spawn3. **Closed 2026-07-09** — T4 recorder `P` flag (`35cd30643`) + T6 six-navmode ranking (live q2dm3, `mode_perf.md`) done; far-spawn quad routes = Plan 35. |
| **44** | **3ZB2-style brain plugin** (`zb2`) | 23/24/25, 43, 46 | pending | **Rewritten 2026-07-09** (old draft referenced nonexistent `world/src/nav_generator.rs` and re-ported 3ZB2's linker — dropped; our graph is richer). Now: `Zb2Brain` plugin à la Plan 37 — committed-route following + `Search_NearlyPod` LOS shortcut-skip over the existing `Navigator`, mover route-states delegated to the Plan 46 traversal executor, weapon-run item bias; competition vs `q3`/`main`. Sequence after 46 and the behavior plans (diversity, not core). |
| **45** | `main` brain competitiveness vs `q3` | 24, 37 | done (stopped at partial win) | **Moved to `completed/` 2026-07-09** (all tracker tasks done; user closed 2026-07-03). Shipped: fire-cadence fix, weapon-rush, weighted item picker, fast strafe juke → main kd 0.47→0.68, deaths 38→25 (not a win over q3 ~1.3; residual gap is per-engagement combat quality). Findings + reverted experiments feed Plans 28/29/30. |
| **46** | **Shared traversal executor** (ladder/swim/ride parity for ALL brains) | 40, 43, 24/25 | done | **Closed 2026-07-09.** One `brain::traverse::TraversalExecutor` (gates()/apply(), recorder `S`/`P`/`L`); runtester/main/q3 all delegate, duplicates deleted. Live matrix: q2dm3 ride — runtester 4/4, main reaches (77 `P`, 0 `P`+`R`), q3 reaches (was 0); q2dm1 swim — **runtester/main/q3 all 3/3** (q3 gained swim from zero). main gained ladders + stateful ride; q3 gained ALL traversal. |
| **47** | **Human-like play acceptance suite** (capstone) | 35, 42, 43, 46, 27–33, (44) | pending | **New 2026-07-09.** One command (`just acceptance` / `tools/acceptance.rs`) runs the traversal matrix per brain (q2dm1 swim, q2dm3 train+lift railgun ≥3/4, q2dm3 ladder+train quad ≥3/4, q2dm2 s2s 8/8) + 5-min competitions with behavior counters (switch-at-range, hurt-pickups, chase conversions, third-party breaks, rides/swims/climbs, drownings=0); baseline + regression contract recorded in `context/acceptance.md`; q2dm3 roster showcase. Sequence LAST. |
| **20** | Hybrid navigation modes | 14, 10 | done | Four `hybrid-*` `--mode` backends combining the A* waypoint graph + navmesh, selectable alongside the untouched `astar`/`navmesh` controls: `hybrid-fallback` (A* primary, navmesh on stuck), `hybrid-race` (plan both, run winner), `hybrid-hier` (navmesh corridor + A* local), `hybrid-segment` (navmesh open + A* jump links). Thin `Navigator` supervisors over both sub-drivers (`brain::hybrid`); one `build_navigator` factory wires both dispatch sites. Code complete + unit-tested; **live A/B against the Plan 10 baselines still pending** (needs a running server). |
| **36** | Quake 3 character + aggression core | 23, 06 | done | Port Q3's personality + decision scalars into a pure, unit-tested `brain::q3char`: `Q3Character` (named [0,1] traits, presets, skill mapping), `bot_aggression`/`bot_feeling_bad` (`ai_dmq3.c:2199`, the engage/disengage scalar) adapted to wire-visible inventory (held-weapon proxy via `Weapon::from_view_model` + `SelfState.held_weapon`), `Weapon::power_tier()`. **Closed 2026-06-19** — 12 unit tests green, additive-only (MainBrain/Sentry/RunTester untouched). Research: `context/distilled/quake3.md` §2–3. |
| **37** | Quake 3 brain plugin (`q3`) | 36, 24, 25 | done | `Q3Brain` (`BrainKind::Quake3`, `--brain q3`) — Q3's node FSM (Seek_LTG/NBG, Battle_Fight/Chase/Retreat/NBG; `ai_dmnet.c`) with aggression-gated retreat/chase, Q3 enemy selection (alertness range, awareness FOV, LOS, sneak-past), and the Q3 aim/fire model (per-weapon accuracy, reaction-time sight gate, fire-throttle duty cycle, radial ground-aim, self-preservation abort, circle-strafe + jump dodge). Reuses `Navigator`/`steer`/`recover`/`los`; injected nav, no `MainBrain` fork. **Closed 2026-06-19** — live q2dm1 A/B: q3 K/D 2.00 vs main 0.75; pure-q3 fleet 9 frags/90s, 0 panics/kicks. Needed a Q2 **blaster-floor** in `bot_aggression` (healthy bot engages on the start weapon). Research: `context/distilled/quake3.md` §1, §4–7. |
| **38** | Quake 3 personality roster + tuning | 37, 21 | done | Turn `q3` into a selectable roster of named Q3 characters (`--q3char`/`[fleet].q3char`/`competition --q3chars`) with distinct skins/names. **Closed 2026-06-19** — `Q3CharPreset` (grunt/major/sarge/camper) threaded through `build_brain`/CLI/config/competition; live q2dm1 tuning shows an intentional spread (major K/D 5.00, sarge 1.25, camper 1.00, grunt 0.00) so presets stand as-is. Observed-inventory upgrade (T3) **deferred** — the Plan 37 blaster-floor already makes held-weapon aggression competitive. Reference shapes: `vendor/Quake-III-Arena/.../bots/*.c` (distilled, not committed). |

**Milestones**
- After **03**: a bot connects and shows in the server's player list.
- After **04**: a bot stands on the map and moves.
- After **05**: qbots can trace and navigate the world like a gamecode bot could.
- After **06**: a single bot plays deathmatch.
- After **07**: a single bot aims, leads, dodges, and tunes skill like an Eraser bot.
- After **08**: bots route strategically — avoiding observed death-traps and gravitating to busy lanes.
- After **09**: a full bot fleet fills a server.
- After **10**: we can *measure* how well a bot moves (per-frame telemetry + elapsed time) — the lens for all movement work.
- After **11**: bots only react to enemies they can actually see.
- After **12**: bots steer along paths the way a human does (turn, then go; no orbiting).
- After **13**: bots recover from wedges and steer around walls instead of grinding into them.
- After **14**: routes are short and smooth via string-pull; spawns are seeded and connected; jump-down ledges navigable.
- After **15**: `spawn-to-spawn` scenario reaches the goal — all nav-quality work from 11–14 is actually exercised in the scenario.
- After **17**: BSP/collision parsing and the nav graph's step-climb threshold are vendor-correct
  and pinned by regression tests.
- After **18**: nav graph generation happens once per map, ahead of time, not once per bot.
- After **19**: a full 8-bot fleet reliably reaches a spawn-to-spawn or spawn-to-weapon goal
  live, on q2dm1, within an extended (60s) timeout — the concrete deliverable this plan series
  was reorganized around on 2026-06-16.
- After **23**: decision-making is a *plugin contract* (`trait Brain`), not a single struct.
- After **24**: `main` is one brain plugin among several; the seam is proven to run >1 brain.
- After **25**: brain and nav backend are independent per-bot axes (`--brain` × `--navmode`).
- After **26**: `spawn-to-*` run on a dedicated, selectable `runtester` brain; the inline
  scenario duplication is gone (Plan 22 T4 closed); all 6 navmodes still match `mode_perf.md`.
- After **27–33**: bots make real persona-driven tactical decisions (weapon matchups,
  third-party disengage, resource seeking, elevators, underwater, danger preference).
- After **36**: Quake 3's aggression scalar + character personality exist as pure, tested
  `brain::q3char` primitives (no brain yet).
- After **37**: a full Quake-3-derived brain (`--brain q3`) connects and plays deathmatch with
  Q3's node FSM, aggression-gated retreat/chase, and Q3 aim/fire texture — a sibling to `main`.
- After **38**: `q3` is a roster of recognizably different, tuned Q3 personalities fielded via
  `--q3char`/competition.

- After **39**: the A* nav graph represents water — q2dm1's railgun room fuses into the main
  component and an A* path from a spawn to the railgun exists (offline-verifiable).
- After **40**: bots swim the water route — dive, swim the tunnel, surface, and climb onto
  the railgun ledge; `spawn-to-weapon railgun` reaches on A*-backed navmodes, with a
  six-navmode ranking recorded.
- After **41**: `spawn-to-item <item>` exists with friendly aliases, and multi-instance
  targets (q2dm3's two railguns) are selectable via `--instance`.
- After **42**: q2dm3's quad and loop-train railgun fuse into the reachable nav graph via
  `func_train` ride edges; `generate-map-cache q2dm3` succeeds.
- After **43**: bots ride q2dm3's moving platforms + railgun elevator — `spawn-to-item
  quaddamage` and `spawn-to-weapon railgun --instance 1` reach, with a six-navmode q2dm3
  ranking recorded.
- After **46**: EVERY brain (`main`, `q3`, future `zb2`) climbs ladders, swims, and rides
  platforms/lifts in live matches — traversal is a shared executor, not a per-brain copy.
- After **35 (revised)**: every route A* returns is physically walkable (hull-valid), and
  the residual map gaps close — bots reach ladder/train/lift-gated items from ANY spawn.
- After **28–30**: bots read the matchup (enemy weapon, ideal range), chase for the kill,
  break third-partied fights, and detour for health/ammo they *know* is on the map.
- After **31–32**: lifts are used politely in crowds (penalty hack deleted) and nobody drowns.
- After **47**: "human-like" is a *measured, repeatable acceptance run* with a recorded
  baseline — the series goal made testable.

> **Brain-notes discipline (Plans 23–33, 36–38, 40, 43):** every brain plan appends a dated section to
> `context/brain_notes.md` (running log, same shape as `map_errors.notes.log.md`). It is a
> verification-checklist item in each brain plan — not optional.

> Active plans live alongside this file as `NN_name.md` + `NN_name_tracker.md`.
> Completed plans move to `context/plans/completed/` (see `RULES.md`).
> Plans that were superseded before completion move to `context/plans/abandoned/` with a short
> note on why and what superseded them.
