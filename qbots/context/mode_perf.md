# Nav `--navmode` performance — q2dm1 sweep

> **Plan 26 (2026-06-18) — runtester-brain acceptance sweep: PASSED.** The `spawn-to-*` decision
> tick was moved out of `scenario.rs` into `RunTesterBrain` (default `--brain runtester`) as a
> **verbatim** lift. Live re-sweep on q2dm1 (default `--brain runtester`, `--count 6` — server
> maxclients=8; per-scenario, not the 16-bot baseline), reach counts:
>
> | navmode | s2s (/6) | s2w RL (/6) | baseline s2s (/16) | baseline s2w (/16) |
> |---|:--:|:--:|:--:|:--:|
> | astar | 5/6 | 6/6 \* | 16/16 | 12/16 |
> | navmesh | 2/6 | 6/6 | 5/16 | 15/16 |
> | hybrid-fallback | 6/6 | 4/6 | 14/16 | 12/16 |
> | hybrid-race | 5/6 | 6/6 | 15/16 | 16/16 |
> | hybrid-hier | 3/6 | 0/6 | 11/16 | 1/16 |
> | hybrid-segment | 4/6 | 3/6 † | 13/16 | 4/16 |
>
> **Zero panics across all 12 runs** (the `hybrid-hier` no-panic gate holds). Every navmode
> reproduces the baseline pattern; mean speeds stayed grounded-realistic (~180–270 u/s).
> \* astar s2w: 3/6 then 6/6 on a re-run — spawn-draw variance at n=6. † hybrid-segment s2w:
> 0/6 at a 55 s cap → 3/6 at the full 180 s cap (time-limited, not a regression). The lift is
> faithful: same code, same `Navigator`, no nav-behaviour change.

**Date:** 2026-06-18 · **Server:** `noir.lan:27910` (q2dm1, 64 slots) ·
**Nav graph:** 12 890 nodes / 519 653 edges (cached, spacing 24) ·
**Bots:** 16 per mode · **Cap:** `--max-secs 180` (3 min/bot) · **Combat:** disabled.

Two Plan 10 movement scenarios, run once per mode with `--count 16 --max-secs 180`:
- `spawn-to-spawn` — drive to the farthest DM spawn from a random spawn.
- `spawn-to-weapon rocketlauncher` — drive to the RL pickup origin.

> ⚠️ **Single sample, high variance.** Each bot gets a *random* spawn, so goal distance/difficulty
> differ per bot. Reach-counts are directional, not exact. `mElaps` = mean elapsed **over bots
> that reached**; other columns average **all 16** bots. High `hindered`/`wrongT` on a low-reach
> mode mostly means "bots flailed for the full 180 s," so those columns inflate when a mode fails.

---

## spawn-to-spawn (180 s cap)

| mode | reached | mElaps (s) | mSpeed | mMax | bumps | wrongT | hinder |
|------|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| **astar**       | **16/16** | 25.1 | 184 | 347 | 22.5 | 28.9 | 65.5 |
| navmesh         | 5/16  | 34.1 | 79  | 319 | 75.9 | 62.0 | 533.4 |
| hybrid-fallback | 14/16 | 43.1 | 158 | 342 | 43.4 | 67.7 | 99.1 |
| **hybrid-race** | 15/16 | 29.1 | 159 | 334 | **17.9** | 37.6 | **38.1** |
| hybrid-hier     | 11/16 | 46.1 | 174 | 358 | 62.8 | 317.2 | 145.6 |
| hybrid-segment  | 13/16 | 33.2 | 193 | 331 | 31.5 | 274.2 | 58.9 |

## spawn-to-weapon rocketlauncher (180 s cap)

| mode | reached | mElaps (s) | mSpeed | mMax | bumps | wrongT | hinder |
|------|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| astar           | 12/16 | 46.4 | 175 | 344 | 96.8 | 90.0 | 311.6 |
| navmesh         | 15/16 | 31.7 | 177 | 314 | 35.0 | 27.7 | 107.3 |
| hybrid-fallback | 12/16 | 62.1 | 173 | 340 | 121.1 | 108.8 | 304.7 |
| **hybrid-race** | **16/16** | 40.2 | 182 | 315 | **22.1** | **25.1** | **56.2** |
| hybrid-hier     | 1/16  | 7.4  | 140 | 336 | 153.2 | 488.2 | 543.5 |
| hybrid-segment  | 4/16  | 22.5 | 129 | 338 | 115.0 | 368.2 | 246.4 |

## Combined reach (/32) and the 30 s → 180 s delta

| mode | s2s | s2w | total | s2s@30s | s2w@30s | what the extra time revealed |
|------|:--:|:--:|:--:|:--:|:--:|---|
| **hybrid-race** | 15 | 16 | **31** | 6 | 7 | best generalist; near-perfect *and* cleanest |
| astar          | 16 | 12 | 28 | 13 | 3 | s2w was mostly slowness, not inability |
| hybrid-fallback| 14 | 12 | 26 | 6 | 4 | tracks astar; pays a churn tax |
| navmesh        | 5  | 15 | 20 | 2 | 9 | s2s is a genuine weakness, not slowness |
| hybrid-segment | 13 | 4  | 17 | 5 | 1 | s2s ok with time; s2w stays broken |
| hybrid-hier    | 11 | 1  | 12 | 2 | 1 | s2s ok with time; s2w stays broken |

---

## Findings

1. **`hybrid-race` is the standout — it wins on reach *and* quality.** 31/32 reaches, the only
   mode strong in both scenarios (15, 16). It isn't just "reaches more": it has the **lowest
   bumps, wrong-turns, and hindered frames in both scenarios** (s2w: 22 bumps / 25 wrongT /
   56 hinder vs astar's 97 / 90 / 312). Because the supervisor scores both plans and runs the
   cheaper one, it inherits whichever backend is actually working — clean A* on the open s2s
   route, clean navmesh on the tight RL pickup — and avoids both backends' flailing. On s2w its
   metrics essentially match pure navmesh; on s2s they beat pure astar.

2. **More time exposes which failures were *slowness* vs *inability*.** At 30 s the picture looked
   like sharp specialists; at 180 s most "failures" healed: astar s2w 3 → 12, race s2w 7 → 16,
   hier s2s 2 → 11. Two genuine weaknesses survived the extra time:
   - **navmesh on `spawn-to-spawn` (5/16, hinder 533).** The long open route is exactly where the
     funnel meanders — bots grind walls (76 bumps) for the full 3 min. Not a speed problem.
   - **hier & segment on the RL goal (1/16, 4/16).** Genuinely can't get there.

3. **`hybrid-hier` and `hybrid-segment` have a real handoff-thrash defect.** Their `wrong_turns`
   are an order of magnitude worse than the leaders (hier 317/488, segment 274/368 vs ~25–40):
   the sliding sub-goal (hier) and jump-link backend flips (segment) make the bot reverse and
   re-aim repeatedly. They reach on the *easy* s2s route given time, but collapse on the tighter
   RL goal. These cooperative designs need debouncing/hysteresis on the backend switch before
   they're competitive.

4. **`hybrid-fallback` pays a measurable churn tax.** It reaches like astar (14/12) but with
   markedly higher bumps (43/121 vs astar's 23/97): each stuck-triggered switch-and-reseed costs
   a few wall scrapes. Useful as a safety net, but the reactive switch isn't free.

5. **`max_speed` 314–358 is brief airborne (jump +270 z), not a physics violation** — no mode
   shows *sustained* >320 grounded speed. hier's 358 is the highest, consistent with its extra
   re-aim jumps.

6. **Bug found & fixed during this sweep.** `hybrid-hier`'s first 180 s run **panicked** —
   "overflow when subtracting durations" in the tracing dedup layer (`elapsed` sampled before the
   lock → a stale-earlier value underflowed the `Duration` sub under 16-bot concurrent logging,
   poisoning the layer mutex and crashing the run). Fixed with `saturating_sub` (+ regression
   test); the re-run above is post-fix. The bug was latent in *all* modes — heavy log churn (hier's
   wrong-turn spam) just made it hit there first.

## Practical takeaway

`hybrid-race` is the best default for a mixed-goal deathmatch fleet: one extra plan per goal buys
both reach and clean motion across goal types. `astar` is fine if goals are mostly open
traversals; `navmesh` if mostly tight item pickups. `hybrid-fallback` is an acceptable safety net.
`hybrid-hier`/`hybrid-segment` are **not yet competitive** — fix the backend-switch thrash first.

## Reproduce

```bash
cargo build -p qbots
for m in astar navmesh hybrid-fallback hybrid-race hybrid-hier hybrid-segment; do
  ./target/debug/qbots spawn-to-spawn               --count 16 --max-secs 180 --navmode "$m" --name "s2s_$m"
  ./target/debug/qbots spawn-to-weapon rocketlauncher --count 16 --max-secs 180 --navmode "$m" --name "s2w_$m"
done
# Per-bot logs: ./logs/<scenario>/<ts>.<bot>.log ; each ends in a `# SUMMARY …` line.
```
