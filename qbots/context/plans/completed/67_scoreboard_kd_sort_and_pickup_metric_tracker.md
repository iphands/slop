# Scoreboard K/D-first ranking + health/armor pickup metric — Tracker

## Overview
- Status: 100% complete
- Start date: 2026-07-13 · Closed: 2026-07-13
- Scope: competition scoreboard sort + new hp/ap measured axis

## Resume Instructions
Done — nothing to resume. Live-verified 2026-07-13 on noir.lan:27910 (q2dm1, 8 bots,
3.5 min): 16 `EVT pickup` events (armor shards +2, health kits +25); EVT sums
(health 144, armor 22) exactly match the FINAL board's hp/ap columns
(`q3_as hp=124 ap=16`, `mai_as hp=20 ap=6`); board ranked by K/D.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: K/D-first scoreboard ranking | `crates/qbots/src/supervisor.rs` | done | `ModeScore::kd()`; kd desc → kills desc → tag; tests updated + tiebreak test. `37969e5f5` |
| 2 | T2: BotTally pickup fields + recorders | `crates/qbots/src/stats.rs` | done | Committed with T3 (recorders alone = dead-code warning, Rule A). `c78e314f2` |
| 3 | T3: pickup detection in bot_task | `crates/qbots/src/main.rs` | done | Hooked the Plan 51 delta block (block 2's heal branch is dead while block 1 runs); `last_armor` + map-change reset; debug `EVT pickup`. `c78e314f2` |
| 4 | T4: scoreboard + final-stats columns | `crates/qbots/src/supervisor.rs` | done | `hp=`/`ap=` on live+FINAL board and `log_final_stats`; aggregation asserted in tests. `c11c5e567` |

## Verification
- [x] T1: board ranks kd 3.0 > 1.0 > 0.5 > 0.0; kills breaks a kd tie (unit tests)
- [x] T2: totals fold sums pickup fields (unit test)
- [x] T3: live EVT spot-check — 16 events, both kinds, respawns NOT counted (deaths=12 in run 2, hp stayed = EVT sum)
- [x] T4: live board renders `hp=`/`ap=`; sums match EVT totals exactly
- [x] fmt/clippy/full-workspace tests green at every commit
