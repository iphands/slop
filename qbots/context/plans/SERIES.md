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
| **19** | Nav graph quality & 8-bot fleet reach validation | 17, 18, 15 | pending | Closes the user-facing goal: `spawn-to-spawn --count 8 --max-secs 60` and `spawn-to-weapon <weapon> --count 8 --max-secs 60` both reach 8/8 live on q2dm1. Seeds scenario goal positions (not just DM spawns) into the nav graph, adds the missing `--max-secs`/`--count` CLI flags, adds a per-bot pass/fail summary. |
| **21** | Competition runner | 09, 20 | done | `qbots competition` spawns N bots per nav `--mode` in one process (shared `NavCache`), one distinct skin per mode, and prints a per-mode frag scoreboard (`FleetStats` grouped by name prefix). In-process (no 6× nav rebuild); makes `mode` per-bot in the fleet supervisor. |
| **22** | Brain seam extraction | 06, 07, 12, 13 | done | Extracted the dissolved decision/steering body of `bot_task` into a single `brain::Brain` (owns combat/fsm/danger/steering/recovery/skill/roam; `Navigator` injected per tick). `bot_task` is now thin orchestration (−~280 net lines in `main.rs`). Verbatim lift; validated live via `connect-one` (full combat+nav+FSM pipeline through `brain.tick`). Nav + driver untouched. **T4 deferred → Plan 23**: migrate `scenario.rs` onto `Brain` (retires the Plan 15 duplication). |
| **23** | Brain plugin **core** (`trait Brain`) | 22 | done | Turn the single concrete `Brain` into a plugin contract: `trait Brain` + `BrainContext`/`BrainOutput`/`BrainConfig`/`BrainMap` + `BrainKind` enum + `build_brain` factory (mirrors `NavMode`/`build_navigator`) in `brain::brains::core`; existing brain implements it behind `Box<dyn Brain>`. Establishes `context/brain_notes.md` (append-on-every-brain-plan rule). **Behavior-preserving.** Supersedes the old single "Behavior/persona" Plan 23, which is now expanded into 23–32. |
| **24** | `main` brain plugin | 23 | done | Relocate the concrete decision body into `brains/main::MainBrain`; add a minimal `SentryBrain` reference plugin (proves the seam runs with >1 brain); `build_brain` dispatches both. `main` behavior byte-identical. (Scenario migration / Plan 22 T4 moved to Plan 26.) |
| **25** | Multibrain selection + `--navmode` rename | 24 | done | `--brain <kind>` on connect-one/run/spawn-*/competition + per-bot `[fleet].brain` config; **any brain × any navmode** (orthogonal axes); rename `--mode`→`--navmode`/`--modes`→`--navmodes` (CLI/help/README/`mode_perf.md`, keep `NavMode` type); `competition --brains` matrix. |
| **26** | `runtester` scenario brain | 25 | pending | Lift `scenario.rs`'s non-combat pathfinding tick (verbatim) into `RuntesterBrain` (`BrainKind::Runtester`); `spawn-to-*` drive a selectable `Box<dyn Brain>` (default `runtester`, `main` for A/B); `goal_override` moves to `BrainContext`; delete inline duplication — **closes Plan 22 T4 + retires Plan 15**. Gated by CI determinism tests + a live **6-navmode** sweep ≥ `context/mode_perf.md` baseline. |
| **27** | Persona parameters (behavior) | 25 | pending | Expand `Personality`/`BotSkill` into a real per-bot persona — aggression, weapon-pref, follow-or-not, reaction, risk tolerance — wired from config/competition; `main` consumes. (Ex-Plan 23 persona.) |
| **28** | Tactical weapon-matchup reads (behavior) | 27 | pending | Infer enemy weapon (PVS-limited observation); **back-up-vs-SSG**, don't-engage-blaster-vs-railgun, per-weapon ideal distance; replace fixed `BACKUP_DIST`/`IDEAL_DIST` with persona+weapon-tuned tactics. |
| **29** | Engagement: chase / disengage / third-party (behavior) | 28 | pending | Chase-or-not by health/weapon/dist; **break a 1v1 when third-partied** (taking fire from afar → make them choose); target prioritization. |
| **30** | Resource decisions: health & ammo (behavior) | 28 | pending | Nearest-health-when-hurt; weapon/ammo need-awareness folded into the item value model. |
| **31** | Elevator / plat behavior (behavior) | 24 | pending | Brain decides→waits-clear→rides via movement intents; **remove `ELEVATOR_PENALTY`** (nav exposes plat top/bottom facts only). Folds `context/elevator_todo.md`; ex-Plan 23 elevator. |
| **32** | Underwater & breath (behavior) | 24 | pending | Dive for an item, monitor air/breath (playerstate water level), surface to breathe, exit-water routing (mostly brain, minimal nav). |
| **33** | Heatmap preference pull-up (behavior) | 24 | pending | Nav exposes per-node danger; **Brain** owns the persona-weighted danger/crowd *preference* instead of A* pricing it. Ex-Plan 23 heatmap. |
| **34** | q2dm3 nav: diagnostics + resilient cache batch | 17, 18 | done | Tooling + unblock: `navinspect QBOTS_LIVE=1` (inspect a map that fails the gate), compgaps flat-gap walkability fix, q2dm3 fragmentation diagnosis, and `generate-map-cache --allow-failures` (caches good maps, names failures). Diagnosis found the regression is **broad (5/8 stock maps fail)**, not a q2dm3 quirk → deep nav fix split to Plan 35. |
| **35** | q2dm nav connectivity regression (5/8 maps) | 34 | pending | Root-cause + fix the regression failing `check_spawn_connectivity` on q2dm2/3/5/6/7: suspect `walkable_stair` floor-check (`662580e69`) over-rejecting real cross-floor stairs + `BRIDGE_HDIST` 512→256 cut + missing z-band tread sampling + lift anchoring. Bisect-driven; restore full spawn connectivity (q2dm7 may stay 5/6). |
| **20** | Hybrid navigation modes | 14, 10 | done | Four `hybrid-*` `--mode` backends combining the A* waypoint graph + navmesh, selectable alongside the untouched `astar`/`navmesh` controls: `hybrid-fallback` (A* primary, navmesh on stuck), `hybrid-race` (plan both, run winner), `hybrid-hier` (navmesh corridor + A* local), `hybrid-segment` (navmesh open + A* jump links). Thin `Navigator` supervisors over both sub-drivers (`brain::hybrid`); one `build_navigator` factory wires both dispatch sites. Code complete + unit-tested; **live A/B against the Plan 10 baselines still pending** (needs a running server). |

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

> **Brain-notes discipline (Plans 23–33):** every brain plan appends a dated section to
> `context/brain_notes.md` (running log, same shape as `map_errors.notes.log.md`). It is a
> verification-checklist item in each brain plan — not optional.

> Active plans live alongside this file as `NN_name.md` + `NN_name_tracker.md`.
> Completed plans move to `context/plans/completed/` (see `RULES.md`).
> Plans that were superseded before completion move to `context/plans/abandoned/` with a short
> note on why and what superseded them.
