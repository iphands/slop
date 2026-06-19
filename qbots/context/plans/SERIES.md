# Plan Series — Dependency Chain

Master ordering of all qbots plans. **Update this file whenever a plan is added,
starts, or completes.** Status values: `pending` | `in-progress` | `done` | `blocked` | `skipped`.

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
| **27** | Persona parameters (behavior) | 25 | pending | Expand `Personality`/`BotSkill` into a real per-bot persona — aggression, weapon-pref, follow-or-not, reaction, risk tolerance — wired from config/competition; `main` consumes. (Ex-Plan 23 persona.) |
| **28** | Tactical weapon-matchup reads (behavior) | 27 | pending | Infer enemy weapon (PVS-limited observation); **back-up-vs-SSG**, don't-engage-blaster-vs-railgun, per-weapon ideal distance; replace fixed `BACKUP_DIST`/`IDEAL_DIST` with persona+weapon-tuned tactics. |
| **29** | Engagement: chase / disengage / third-party (behavior) | 28 | pending | Chase-or-not by health/weapon/dist; **break a 1v1 when third-partied** (taking fire from afar → make them choose); target prioritization. |
| **30** | Resource decisions: health & ammo (behavior) | 28 | pending | Nearest-health-when-hurt; weapon/ammo need-awareness folded into the item value model. |
| **31** | Elevator / plat behavior (behavior) | 24 | pending | Brain decides→waits-clear→rides via movement intents; **remove `ELEVATOR_PENALTY`** (nav exposes plat top/bottom facts only). Folds `context/elevator_todo.md`; ex-Plan 23 elevator. |
| **32** | Underwater & breath (behavior) | 24 | pending | Dive for an item, monitor air/breath (playerstate water level), surface to breathe, exit-water routing (mostly brain, minimal nav). |
| **33** | Heatmap preference pull-up (behavior) | 24 | pending | Nav exposes per-node danger; **Brain** owns the persona-weighted danger/crowd *preference* instead of A* pricing it. Ex-Plan 23 heatmap. |
| **34** | q2dm3 nav: diagnostics + resilient cache batch | 17, 18 | done | Tooling + unblock: `navinspect QBOTS_LIVE=1` (inspect a map that fails the gate), compgaps flat-gap walkability fix, q2dm3 fragmentation diagnosis, and `generate-map-cache --allow-failures` (caches good maps, names failures). Diagnosis found the regression is **broad (5/8 stock maps fail)**, not a q2dm3 quirk → deep nav fix split to Plan 35. |
| **35** | q2dm nav connectivity regression (5/8 maps) | 34 | pending | Root-cause + fix the regression failing `check_spawn_connectivity` on q2dm2/3/5/6/7: suspect `walkable_stair` floor-check (`662580e69`) over-rejecting real cross-floor stairs + `BRIDGE_HDIST` 512→256 cut + missing z-band tread sampling + lift anchoring. Bisect-driven; restore full spawn connectivity (q2dm7 may stay 5/6). |
| **39** | Water nav graph (swim nodes + swim edges) | 05, 17, 18 | done | A* `NavGraph` now traverses water: `EdgeKind::Swim`, submerged/surface node sampling (`water_waypoints_multi`), 3-D swim↔swim + dry↔water entry/exit edges (`try_swim_edge`), prune-protection, cache v13, `navinspect contents/watermap/gpath`. **Closed 2026-06-19** — q2dm1 railgun `(240,-384,464)` joins the main component; `gpath` from both baseline spawns returns a path (8 swim edges). Synthetic + gated q2dm1 reachability tests green. Navmesh water deferred (a follow-up). |
| **40** | Swim movement, water-exit & navmode ranking | 39, 10, 12, 13, 26 | done | Brain executes swim edges: `brain::water::water_level` (recomputed from `cm`+origin), sustained `intent.up`+pitch thrust, Q2 water-jump climb-out, recovery suspended while swimming, recorder `S`. **Closed 2026-06-19** — live q2dm1: `spawn-to-weapon railgun` `reached=true` on `astar` (~11–27 s; 46/93 frames `S`, z 238→434 = dive→surface). Navmode ranking: **4/6 reach** (astar + all A*-backed hybrids); pure-`navmesh` + `hybrid-segment` don't (no navmesh water). See `context/mode_perf.md`. |
| **41** | `spawn-to-item` + item/weapon target resolution | 10, 26 | done | `spawn-to-item <item>` + friendly aliases (`quaddamage`→`item_quad`) + `--instance N` (q2dm3 has **two** `weapon_railgun`). Verified: quad→`(192,320,216)`, railgun-1→`(768,816,208)`. Moved to `completed/`. |
| **42** | Moving-platform (`func_train`) + lift nav integration (q2dm3) | 17, 18, 39, 34, 35 | mostly-done | `EdgeKind::Ride`+`RideInfo`, `func_train` ride edges (ground-anchored boards), `func_plat`/`func_door`→**vertical ride edges**, `bridge_components_via_jump` (stacked-floor drops), cache **v16**. **q2dm3 railgun A\*-reachable from all 7 spawns**; `generate-map-cache q2dm3 --allow-failures` writes the cache. **Quad NOT reachable** — upper level (comp0) has no spawn-side up-route → **blocked on Plan 35** (broad floor fragmentation). |
| **43** | Moving-platform & lift **ride behavior** + q2dm3 reach proof / navmode ranking | 42, 41, 40, 26 | in-progress | Brain `Ride` execution in `MainBrain`+`RunTesterBrain`: approach/wait/board/ride/dismount, recovery suspended, **JUMP on/off** (T7, user insight), train detection via wire-origin (`board_ent`) + **live top-center tracking** while carried. **q2dm3 railgun REACHED — astar 3/4, hybrid-race 3/4** (`mode_perf.md`); lifts + trains both ridden. **Remaining:** the **quad** is nav-unreachable (upper-level fragmentation → **Plan 35**); recorder `P` flag (T4) deferred. |
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

> **Brain-notes discipline (Plans 23–33, 36–38, 40, 43):** every brain plan appends a dated section to
> `context/brain_notes.md` (running log, same shape as `map_errors.notes.log.md`). It is a
> verification-checklist item in each brain plan — not optional.

> Active plans live alongside this file as `NN_name.md` + `NN_name_tracker.md`.
> Completed plans move to `context/plans/completed/` (see `RULES.md`).
> Plans that were superseded before completion move to `context/plans/abandoned/` with a short
> note on why and what superseded them.
