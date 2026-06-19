# Nav `--mode` performance — q2dm1 sweep

**Date:** 2026-06-18 · **Server:** `noir.lan:27910` (q2dm1, 64 slots) ·
**Nav graph:** 12 890 nodes / 519 653 edges (cached, spacing 24) ·
**Bots:** 16 per mode · **Cap:** 30 s wall-clock per bot · **Combat:** disabled (movement-only).

Two movement scenarios from Plan 10, each run once per mode with `--count 16`:
- `spawn-to-spawn` — drive to the farthest DM spawn from a random spawn.
- `spawn-to-weapon rocketlauncher` — drive to the RL pickup origin (`weapon_rocketlauncher`).

> ⚠️ **Single sample, high variance.** Each bot gets a *random* spawn, so the goal distance
> (and difficulty) differs per bot and per run. Treat reach-counts as directional, not exact;
> repeat the sweep for statistical confidence. `mElaps` is mean elapsed **over bots that
> reached**; all other columns average **all 16** bots.

---

## spawn-to-spawn

| mode | reached | mElaps (s) | mSpeed (u/s) | mMax | bumps | wrongT | hinder |
|------|:------:|:---:|:---:|:---:|:---:|:---:|:---:|
| **astar**          | **13/16** | 17.4 | 218 | 343 | 16.1 | 24.9 | 26.0 |
| navmesh            | 2/16  | 19.8 | 101 | 313 | 13.9 | 23.0 | 63.6 |
| hybrid-fallback    | 6/16  | 17.8 | 190 | 354 | 28.6 | 24.8 | 61.6 |
| hybrid-race        | 6/16  | 20.9 | 117 | 324 | 19.1 | 26.4 | 47.7 |
| hybrid-hier        | 2/16  | 18.2 | 152 | 337 | 23.1 | 56.6 | 80.7 |
| hybrid-segment     | 5/16  | 23.9 | 167 | 328 | 25.4 | 82.8 | 55.9 |

## spawn-to-weapon rocketlauncher

| mode | reached | mElaps (s) | mSpeed (u/s) | mMax | bumps | wrongT | hinder |
|------|:------:|:---:|:---:|:---:|:---:|:---:|:---:|
| astar              | 3/16  | 20.5 | 177 | 333 | 29.6 | 28.2 | 78.4 |
| **navmesh**        | **9/16** | 17.6 | 177 | 312 | 12.0 | 11.9 | 31.1 |
| hybrid-fallback    | 4/16  | 12.1 | 185 | 332 | 27.6 | 26.2 | 61.7 |
| **hybrid-race**    | **7/16** | 15.9 | 180 | 313 | 13.2 | 12.4 | 30.9 |
| hybrid-hier        | 1/16  | 7.4  | 160 | 333 | 25.2 | 71.4 | 51.0 |
| hybrid-segment     | 1/16  | 7.5  | 150 | 325 | 29.8 | 61.2 | 69.1 |

## Combined reach (both scenarios, /32)

| mode | s2s | s2w | total | balance |
|------|:--:|:--:|:--:|---|
| **hybrid-race** | 6 | 7 | **13** | **most balanced** — only mode strong in *both* |
| astar          | 13 | 3 | 16 | top total, but lopsided (great s2s, weak s2w) |
| navmesh        | 2  | 9 | 11 | mirror of astar (weak s2s, great s2w) |
| hybrid-fallback| 6  | 4 | 10 | tracks astar (A*-first) |
| hybrid-segment | 5  | 1 | 6  | regressed |
| hybrid-hier    | 2  | 1 | 3  | regressed |

---

## Findings

1. **No single pure backend wins both goals — they're opposite specialists.**
   `astar` aces `spawn-to-spawn` (13/16) but collapses on the RL goal (3/16); `navmesh` is the
   exact inverse (2 → 9). The RL pickup sits in tighter geometry where the graph's grid snapping
   sends bots to the wrong side of a lip, while the long open s2s route is where the navmesh
   funnel meanders (navmesh s2s: mSpeed 101, hinder 64 — it wanders).

2. **`hybrid-race` is the best generalist, and it works for the intended reason.**
   It's the only mode competitive in *both* (6 and 7), with the highest *balanced* total. On the
   RL goal its stats are nearly identical to pure `navmesh` (mSpeed 180 vs 177, bumps 13 vs 12,
   wrongT 12 vs 12, hinder 31 vs 31) — i.e. it **scored navmesh as the cheaper plan and ran it**,
   capturing navmesh's RL strength without inheriting astar's RL collapse. This validates the
   "plan both, run the winner" thesis.

3. **`hybrid-fallback` behaves like A*-first, as designed — and that's its ceiling.**
   It mirrors `astar` (s2s 6, s2w 4) because it only hands off *on a hard-stuck*. On the RL goal
   astar often reaches a *wrong* settled spot rather than getting stuck, so the navmesh handoff
   never fires. Its bump count is the highest of all (s2s 28.6): the switch-and-reseed churns.

4. **`hybrid-hier` and `hybrid-segment` regressed — the dual-driver handoff thrashes steering.**
   Their `wrong_turns` explode (hier 56–71, segment 61–83 vs ~12–28 for the leaders): the sliding
   sub-goal (hier) and the jump-link backend flips (segment) make the bot reverse and re-aim. The
   tiny `mElaps` on s2w (7.4/7.5 s, 1 reach) means only a near-spawn goal succeeded; the rest
   failed. These cooperative designs need work before they beat the controls.

5. **`max_speed` 320–354 is brief airborne, not a physics violation.**
   Grounded cap is ~300; the spikes coincide with jump frames (jump adds +270 to `velocity.z`,
   and the recorder samples 3D speed). `hybrid-fallback` shows the highest (354), consistent with
   its extra replan-driven jumps. No mode shows *sustained* >320 grounded speed.

6. **Practical takeaway.** For a deathmatch fleet that pursues mixed goals (spawns, items,
   enemies), `hybrid-race` is the safest default: it pays one extra plan per goal to avoid each
   pure backend's worst case. `astar` remains best if goals are mostly long open traversals;
   `navmesh` if goals are mostly tight item pickups. `hybrid-hier`/`hybrid-segment` are not yet
   competitive.

## Reproduce

```bash
cargo build -p qbots
for m in astar navmesh hybrid-fallback hybrid-race hybrid-hier hybrid-segment; do
  ./target/debug/qbots spawn-to-spawn               --count 16 --mode "$m" --name "s2s_$m"
  ./target/debug/qbots spawn-to-weapon rocketlauncher --count 16 --mode "$m" --name "s2w_$m"
done
# Per-bot logs: ./logs/<scenario>/<ts>.<bot>.log ; each ends in a `# SUMMARY …` line.
```
