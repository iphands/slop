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
| **06** | Brain (`brain`) | 05 | in-progress | perceive → navigate → fight → **single bot scores frags** |
| **07** | Fleet (`qbots` bin) | 06 | pending | N supervised bots, staggered, logged, rate-safe |

**Milestones**
- After **03**: a bot connects and shows in the server's player list.
- After **04**: a bot stands on the map and moves.
- After **05**: qbots can trace and navigate the world like a gamecode bot could.
- After **06**: a single bot plays deathmatch.
- After **07**: a full bot fleet fills a server.

> Active plans live alongside this file as `NN_name.md` + `NN_name_tracker.md`.
> Completed plans move to `context/plans/completed/` (see `RULES.md`).
