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
| **08** | Danger/popularity heatmap nav | 05, 06 | done | runtime risk overlay on the static nav graph (novel — Eraser can't) → **route around death-traps, toward hot lanes** (deterministic integration test verifies detour→decay-restore + skill-scaling; live confirm deferred — server down 2026-06-15) |
| **09** | Fleet (`qbots` bin) | 06 | done | roster config, supervisor (shared nav cache, stagger, reconnect/backoff, graceful shutdown), per-bot logging, rate-safe pacing, `run`/`connect-one`/`status` CLI → **8-bot fleet verified live** (qb0-qb7 connected, frags accumulating, no kicks; `qbots status` OOB lens) |

**Milestones**
- After **03**: a bot connects and shows in the server's player list.
- After **04**: a bot stands on the map and moves.
- After **05**: qbots can trace and navigate the world like a gamecode bot could.
- After **06**: a single bot plays deathmatch.
- After **07**: a single bot aims, leads, dodges, and tunes skill like an Eraser bot.
- After **08**: bots route strategically — avoiding observed death-traps and gravitating to busy lanes.
- After **09**: a full bot fleet fills a server.

> Active plans live alongside this file as `NN_name.md` + `NN_name_tracker.md`.
> Completed plans move to `context/plans/completed/` (see `RULES.md`).
