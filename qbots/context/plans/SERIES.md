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
| **13** | Stuck recovery & wall avoidance | 12 | pending | port `botRoamFindBestDirection` fan-out + inline micro-recovery (strafe ±110°/jump/back-off), unified 4u/1s stuck detector, tightened 2s/4s give-up → **bots unstick & steer around geometry** |
| **14** | Nav-graph & path quality | 10 | deferred | funnel/string-pull smoothing, jump-up/down links, spawn-point connectivity, node redundancy pruning → **shorter, smoother, faster routes** (grid-zigzag elimination; elapsed-time gains) |

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
- After **14** (deferred): routes are short and smooth, not grid stair-steps.

> Active plans live alongside this file as `NN_name.md` + `NN_name_tracker.md`.
> Completed plans move to `context/plans/completed/` (see `RULES.md`).
