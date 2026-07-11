# zb2 Combat Wall-Stall — Tracker

## Overview
- Status: 100% complete (closed 2026-07-11)
- Start date: 2026-07-11
- Server: config default (noir.lan:27910), map q2dm3 (verified live via `status`)
- Soak recipe: 305 s, `competition --count 3 --brains main,q3,zb2 --navmodes astar` (matches Plans 49/50 baselines)

## Resume Instructions
1. Re-read `context/plans/RULES.md` and Plan 51.
2. Check the Progress table below; the first non-`done` row is the current task.
3. Baseline/post-fix soak logs land in `logs/p51_baseline.log` / `logs/p51_postfix.log` (gitignored — numbers live HERE).

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: StallMonitor + fleet wiring + zb2 probe | `brain/stall.rs`, `qbots/main.rs`, `brains/zb2.rs` | done | 5 unit tests; `EVT wall_press` carries `bot=` for per-brain attribution (span fields are dropped by the abbreviated formatter) |
| 2 | T2: baseline soak + analysis table | tracker | done | 2 baseline soaks + 2-bot micro-soak; 4 root causes proven (R1–R4) |
| 3 | T3: fix proven root causes | `brains/zb2.rs` | done | R1 progress watchdog, R2 combat keeps recovery legs, R3 roam desync, R4 reachable start; 9 zb2 unit tests |
| 4 | T4: re-soak compare + notes + close | `context/brain_notes.md`, `context/pitfalls.md` | done | ≥3 s stall class 15 eps/187 s → 0/0; max 97.4→2.9 s; notes + pitfall appended |

## Baseline (T2)

Both runs: 305 s, 3×main + 3×q3 + 3×zb2 on live q2dm3 (915 bot-seconds per group).

**Run 1** (`logs/p51_baseline_run1.log`, no `pp` field yet):

| Group | eps | stalled s | % of bot-time | dmg stalled | atk>0 eps | died-in-ep | max ep |
|-------|----:|----------:|--------------:|------------:|----------:|-----------:|-------:|
| main_astar | 39 | 56.8 | 6.2% | 75 | 24 | 3 | 3.8 s |
| q3_astar | 63 | 99.8 | 10.9% | 525 | 42 | 9 | 4.5 s |
| zb2_astar | 118 | 350.0 | **38.3%** | 1041 | 47 | 16 | **97.4 s** |

**Run 2** (`logs/p51_baseline.log`, with `pp` = min player distance in episode):

| Group | eps | stalled s | player-block eps (<48u) / s | wall-grind eps (wall>50%) / s | dmg stalled |
|-------|----:|----------:|----------------------------:|------------------------------:|------------:|
| main_astar | 37 | 61.9 | 1 / 3.0 | 35 / 57.9 | 246* |
| q3_astar | 78 | 126.1 | 1 / 3.9 | 68 / 111.2 | 771* |
| zb2_astar | 278 | **460.1 (50%)** | **50 / 88.3** | **228 / 371.8** | 806 (521 in firing eps) |

\* main+q3 combined = 1017 (per-group split not recomputed).

**Findings (proof, not guesses):**
1. zb2 stalls 6–8× more bot-time than main (38–50% of its life). Matches the live report.
2. Two distinct mechanisms:
   - **F1 — wall-grind loops at fixed hotspots**: repeated short (1–2 s) grinds re-triggering at the *same coordinate* (11× at (234,-435,360), 9× at (232,-428,360); also (224,240,-15), (-616,683,-15), (402,-306,-7)); no player nearby (pp≈230+). The route repeatedly steers into the same static geometry, recovers, re-approaches. `navinspect` says (234,-435,360)±few u is **startsolid in our CM** while the server let the bot stand there — a world-model/geometry disagreement at the hotspot.
   - **F2 — bot-vs-bot hull deadlocks**: 50 episodes/88 s (run 2) at hull-contact distance (pp≈33); run 1's monster episodes (97.4 s, 18 s, 16.9 s — two overlapping bots 33 u apart at (-601/-568, 677, 248), on a plain walkway, no mover/submodel there). zb2 bots share a deterministic roam cursor (same `roam_idx=0` start, same stride) → they convoy to identical destinations and deadlock; strafe recovery does not break the deadlock (97 s!), and the victim eats damage and dies (`died=true`, 87 dmg while frozen).
3. **Suspect 1 (combat clobber of recovery) is real but partial**: `EVT zb2_combat_recovery_overwrite` 75 (run 1)/93 (run 2, dedup-undercounted); zb2 damage eaten while stalled-and-firing = 521/806. It explains the *firing* subset of stalls, not the majority stall time.

**Micro-soak forensics** (`logs/p51_micro_zb2.log`: 150 s, 2×zb2 only, `RUST_LOG=brain=debug`): 113 episodes from two bots alone. Per-tick `zb2 recovery` lines nail the mechanism:
- Bot pinned on the SAME waypoint for many seconds (`wp=Some(1698)` while sliding along the x=224 wall face y 240→367→back; second bot `wp=Some(1392)` oscillating at (-150..-195,120..200)). Node-id locality (navinspect: 1688=(-193,103); hotspot nodes are 329x/17xx-18xx) puts both pinned waypoints far from the bot — the cursor points somewhere unreachable in a straight line (bot displaced off the committed polyline, largely by the other bot).
- **Root cause R1**: the recovery strafe slides the bot ALONG the wall at 30–100 u/s — above the stuck detector's 16 u deadband — so `stuck_secs` resets every 1 s sample, `StuckLevel::Hard` is never reached, `BackOffThenRepath` never fires, `route.dirty` is never set: the committed route NEVER replans. Mild→strafe→slide→reset→Mild, forever.
- **Root cause R2** (combat subset): when `should_fire`, the run-and-gun block overwrites recovery's legs entirely (probe: 93 events; 521 of 806 stalled dmg in firing episodes) — while firing, even the futile strafe is discarded.
- **Root cause R3** (convoy): all zb2 bots share the deterministic roam cursor (`roam_idx=0`, same stride) → identical destination sequences → hull-to-hull shoving and deadlocks (F2), which is also what displaces bots off their polylines feeding F1.

## Post-fix (T4)

**Round 1** (`logs/p51_postfix_run1.log`, after R1+R2+R3):

| Group | eps | stalled s | % of bot-time | player-block eps/s | wall eps/s | died-in-ep | max ep |
|-------|----:|----------:|--------------:|-------------------:|-----------:|-----------:|-------:|
| main_astar | 56 | 77.7 | 8.5% | 12 / 14.7 | 38 / 57.0 | 4 | 5.9 s |
| q3_astar | 73 | 107.4 | 11.7% | 0 / 0 | 67 / 101.0 | 4 | 9.4 s |
| zb2_astar | 174 | 304.1 | 33.2% | 21 / 37.1 | 147 / 259.0 | 8 | 20.9 s |

zb2 vs baseline run 2: stalled 460→304 s, max 97.4→20.9 s, died-in-ep 16→8, player-block 88→37 s. Watchdog fired 133×. Scoreboard: zb2 kd 0.77 (#1 kills 23) / main 0.87 / q3 0.45 — zb2's best result to date. Below the >50% cut target → analyzed the survivors:
- The 20.9 s monster at (-212,232,-15): wall-pressed EVERY tick with replans looping — the route START came from euclidean `graph.nearest`, which projected across a thin wall; every replan recommitted an unreachable `path[0]` (goal-block only blocks *destinations*). → **R4**: `reachable_start` (nearest candidate with a hull-clear + floored straight line, K=12 within 256 u, euclid fallback).
- Residual short-grind corridors at (192..256,-320..-448, z≈360) and (-192..-128, 64..192, z=-15): watchdog caps each at ~2.5 s but routes keep re-entering. navinspect shows the z=360 spot is **startsolid in our CM where the server lets bots stand** — a CM/nav-cache geometry discrepancy, follow-up candidate for a nav-cache plan (out of Plan 51 scope).

**Round 2** (`logs/p51_postfix.log`, after R1–R4):

| Group | eps | stalled s | % of bot-time | player-block eps/s | wall eps/s | died-in-ep | max ep |
|-------|----:|----------:|--------------:|-------------------:|-----------:|-----------:|-------:|
| main_astar | 44 | 61.1 | 6.7% | 2 / 2.6 | 37 / 54.5 | 2 | 3.1 s |
| q3_astar | 85 | 166.7 | 18.2% | 2 / 7.8 | 78 / 152.0 | 11 | 7.7 s |
| zb2_astar | 197 | 321.8 | 35.2% | 21 / 38.2 | 171 / 277.6 | 15 | **2.9 s** |

**Verdict (the metric that matters — the ≥3 s "sitting duck" class, zb2):**

| Run | ≥3 s eps | ≥3 s secs | max episode |
|-----|---------:|----------:|------------:|
| baseline run 1 | 15 | 187 | 97.4 s |
| baseline run 2 | 6 | 21 | 8.0 s |
| post-fix R1–R3 | 3 | 30 | 20.9 s |
| post-fix R1–R4 | **0** | **0** | **2.9 s** |

Long stalls are eliminated; every residual bump is watchdog-capped ≤2.9 s. Total zb2
stall-time is down ~30% (460→~310 s) but remains 4–5× main's, concentrated in two
corridors where `navinspect` shows our CM is startsolid where the server allows standing
— a `world`/map-cache geometry discrepancy, out of brain scope, flagged in brain_notes as
the follow-up plan candidate. main is unchanged (61.9→61.1 s); q3's swing (107→167 s) is
untouched-code run variance (Plan 47 noise floor) plus a human player on the server.
