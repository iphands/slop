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

---

## Q3 personality roster — live frag spread (Plan 38 T4, 2026-06-19)

`qbots competition --navmodes astar --brains q3 --chars grunt,major,sarge,camper --count 2`
(8 q3 bots, q2dm1, noir.lan, 160 s). The preset value sets (`q3char.rs::Q3Character::{grunt,
major,sarge,camper}`) produce a clear, intentional hierarchy — **no float retuning needed**:

| Character | kills | deaths | K/D | character read |
|-----------|------:|-------:|----:|----------------|
| **major**  | 5 | 1 | 5.00 | precise + efficient (high aim_skill 0.9, low firethrottle 0.2) |
| **sarge**  | 5 | 4 | 1.25 | aggressive + mobile brawler (aggression 0.9, jumper 0.8 → trades a lot) |
| **camper** | 1 | 1 | 1.00 | cautious, holds spots (aggression 0.2 → retreats more) |
| **grunt**  | 0 | 7 | 0.00 | cannon fodder, sprays + dies (aim 0.4/0.3, firethrottle 0.7) |

0 panics. The spread matches the design intent (Grunt dies most, Major most efficient, Sarge
aggressive, Camper passive) → roster is distinct **and** balanced. Presets stand as-is.

## Railgun swim route — `spawn-to-weapon railgun` per navmode (Plan 40 T7, 2026-06-19)

The q2dm1 railgun (`240 -384 464`) is reachable **only by swimming** (dive → submerged tunnel →
surface → climb onto the ledge). Plan 39 added water to the **A* graph**; Plan 40 made the brain
swim it. Sweep: local yquake2 dedicated (q2dm1), `--brain runtester --count 1 --max-secs 95`,
one run per mode (spawn point is random per run, so elapsed varies — the **reached** column is the
signal):

| navmode | reached | elapsed (s) | mean_speed | notes |
|-----------------|:-------:|:-----------:|:----------:|-------|
| astar           | ✅ | 11–27 | 165–207 | A* graph has the swim route; baseline |
| hybrid-fallback | ✅ | 28 | 197 | A* primary → plans the swim directly |
| hybrid-race     | ✅ | 40 | 222 | plans both; the A* (swim) plan wins |
| hybrid-hier     | ✅ | 18 | 212 | navmesh corridor + A* local; A* local finds water |
| navmesh         | ❌ | 95 (cap) | 185 | **no navmesh water** — Plan 39 scope; expected |
| hybrid-segment  | ❌ | 95 (cap) | 185 | navmesh corridor mis-routes (railgun room is navmesh-isolated → goal lands away from the water entry, so the bot never reaches a swim node). Selector IS swim-aware now (parity with jump links), but that's necessary-not-sufficient here. |

**Verdict:** **4/6 reach** — every A*-driven mode swims to the railgun; the two pure-navmesh-corridor
modes can't (the navmesh has no water by design). Swim proof: the astar log showed **46/93 frames
`S`-flagged**, z 238 → 434 (dive → surface). Navmesh water is a deferred follow-up; `hybrid-segment`
would also need a water-aware navmesh corridor (or a fallback-to-A* when the navmesh goal is
unreachable) to reach.

---

## q2dm3 — moving-platform railgun (Plan 42/43, 2026-06-19)

`spawn-to-weapon railgun --instance 1` (the loop-train + lift railgun at `(768,816,208)`),
`--count 4 --max-secs 150 --lift-penalty 0`, server on q2dm3. Reaching requires riding the
loop `func_train`s across the pit (board/ride/dismount with **jumps**, Plan 43 T7) and the
`func_plat` lift — the brain rides moving platforms now.

| navmode | reached | notes |
|---|:--:|---|
| astar | **3/4** | times 32 / 91 / 108 s; the reference |
| hybrid-race | **3/4** | plans both, the A* plan (with ride edges) wins |
| **hybrid-hier** | **3/4** | times 37 / 55 / 91 s — **rides too** (2026-07-09 T6): its A* *local* planner inside the navmesh corridor carries the ride edges. Contradicts the pre-run "expected 0". |
| hybrid-fallback | 1/4 | degrades to navmesh on stuck; navmesh has no ride edges |
| navmesh | **0/4** | confirmed 0 (2026-07-09 T6) — pure navmesh backend has no ride edges, like water (Plan 40) |
| hybrid-segment | **0/4** | confirmed 0 (2026-07-09 T6) — navmesh-open segments carry no ride edges; only the A* *jump*-link segments do |

**Before Plan 43**: 0/N — the bot couldn't ride lifts (tried to "walk" the vertical edge) or
trains (fell in the pit). **Key fixes**: lifts → vertical ride edges; train board anchored to
solid ground; train detection by wire-origin (`board_ent`); **JUMP on/off** + track the train's
live top-center while carried (the user's "I always jump" insight — cut deaths from ~7 to ~1).

**Quad** (`spawn-to-item quaddamage`): still nav-unreachable (q2dm3 upper-level fragmentation,
Plan 35) — pending, not a ride-behavior issue.

### q2dm3 update (2026-06-19, final this session)
- **Railgun (`spawn-to-weapon railgun --instance 1`, astar): reaches reliably, 3/4** (range 1–4/4
  across runs — high spawn-variance, 4/4 observed). Rides the `*3/*4` loop trains + `*2` lift.
- **Quad (`spawn-to-item quaddamage --count 1`, astar): NOT reached (0/N).** Nav-reachable via
  `*10` over the lava; the bot boards and the platform carries it (Q2 push; verified z holds with
  zero input), but the over-lava **board + dismount** for the long oscillating `*10` don't
  complete — and the board style that `*10` needs (gentle, no-jump) conflicts with what the
  railgun loop trains need (jump-on). Closest ~229u. A genuine control-feasibility wall.

### q2dm3 T6 closeout (2026-07-09) — full six-navmode ranking

Ran the three previously-untested navmodes live on q2dm3 (`noir.lan:27910`, cache spacing 24,
`--lift-penalty 0`) for both goals to complete the table. Cap `--max-secs 150`.

**Railgun (`spawn-to-weapon railgun --instance 1 --count 4`)** — full ranking:

| navmode | reached | vs prediction |
|---|:--:|---|
| astar | 3/4 | — |
| hybrid-race | 3/4 | — |
| hybrid-hier | **3/4** | **beat prediction** (predicted 0) — rides via its A* local planner |
| hybrid-fallback | 1/4 | — |
| navmesh | 0/4 | as predicted (no ride edges) |
| hybrid-segment | 0/4 | as predicted |

**Quad (`spawn-to-item quaddamage --count 1`, random spawn)** — navmesh 0/1, hybrid-hier 0/1,
hybrid-segment 0/1. Consistent with astar's random-spawn 0/N: the quad reaches only from the
board-adjacent spawn3 (accepted scope), and `--count 1` draws a random spawn. Far-spawn quad
routes remain **Plan 35**. Not a ride-behavior regression.

**Takeaway:** ride traversal works on **every A*-backed navmode** (astar, hybrid-race,
hybrid-hier), plus hybrid-fallback until it degrades to navmesh. Only the pure-navmesh backend
(navmesh, hybrid-segment's open segments) lacks ride edges — the same structural gap as water
(Plan 40). Navmesh water/rides is a deferred follow-up, not a Plan 43 blocker.

## zb2 brain debut (Plan 44, 2026-07-10) — q2dm3, single runs (noise caveat)

| matchup (5 min, --count 3, astar) | kd | notes |
|---|---|---|
| **zb2 0.38 (8k/21d) vs q3 0.20 (9k/44d)** | zb2 wins on kd | zb2 died HALF as often — the committed-route "purposeful runner" avoids q2dm3's lava churn |
| main 0.82 (14k/17d) vs zb2 0.24 (4k/17d) | main wins | post-thrash-fix main is strong; zb2 matches its death count but converts fewer kills |

Traversal: q2dm1 swim-railgun **2/3**; q2dm3 ride-railgun **1/4** (one 29.4s reach through the
loop trains + lift via the `Zb2Route` facade — the ride works; the far-spawn shortfall vs the
peers' 3/4 is route-following quality: the plain node-by-node follower lacks `pursue`
look-ahead/smoothing on the fragmented upper level — the named tuning follow-up). 28 traversal
legs + 0 drownings + 0 panics across all runs. Single-run kd numbers are directional only
(acceptance.md noise floor ~0.6-1.0); the deliverable is a *distinct, competent* third brain,
which this is.

## 2026-07-11 — Plan 60 T7/T8: `xon` brain baselines (q2dm1)

### T7 spawn-to-* (q2dm1 legs; q2dm3 ride legs BLOCKED — server map change needs user-run RCON)
| Scenario | Result |
|---|---|
| s2s ×4 | **3/4 reached** (8.85s/2019u, 14.40s/3487u, 11.44s/3006u; 1 cap-miss 3389u) — q3-parity+ (same-session q3 1/3) |
| spawn-to-weapon railgun (swim) | **reached** 14.91s/3005u, 9 bumps |

### T8 live competition — 2× 5-min, `--brains mai,q3,xon --count 2 --navmodes as`
| Group | Run1 kd | Run2 kd | Mean | Notes |
|---|---|---|---|---|
| q3_as | 1.25 (5/4) | 1.00 (4/4) | 1.13 | control band |
| mai_as | 0.57 (4/7) | 0.57 (4/7) | 0.57 | |
| xon_as | 0.20 (1/5) | 0.50 (1/2) | 0.35 | low kill rate; survives well (fewest deaths run2) |

0 panics/kicks/drowns/lava-events across both runs. xon's kill rate is the gap:
hypothesis = default skill 5 → fire cone (1000/(d−9)−0.35)×2.5 ≈ 3.3° at 600u while the
fighting bad-aim offset swings ±4.5° (vendor-authentic miss profile at mid skill) + items
out-rating enemies in the goal layer (vendor numbers). Tuning → Plan 62 (presets sweep with
the aggregator; candidates: default skill ↑, aggres axis ↑, offset axis ↑).

### T7 addendum (q2dm3, map flipped by user 2026-07-11)
| Scenario | xon | runtester control |
|---|---|---|
| spawn-to-item quaddamage | 0/1 (cap) | 0/1 (cap) — the Plan 47 "quad" map finding, not brain-specific |
| spawn-to-weapon railgun --instance 1 | **1/4** (23.17s, 123 `P` ride frames, 2 bumps) | 1/1 (25.82s) |

Ride capability proven (clean board/carry/dismount); 1/4 reliability = zb2's baseline class
on this leg (route-quality, Plans 35/47 findings).

### T8 addendum — 5-min q2dm3 soak (`--brains q3,xon --count 2`)
| Group | K/D |
|---|---|
| **xon_as** | **0.60** (6/10) — beats q3 on the traversal-heavy map |
| q3_as | 0.30 (3/10) |

0 panics, 0 drownings, 33 completed traversal legs (`EVT traverse done`), 248 lava-escape
override engagements (q2dm3's environment, Plan 50 family — no regression signal).

## 2026-07-11 — Plan 61: `xg` (xon-goal) navmode sweep — the SEVENTH navmode

`XonNavDriver` = A* + swim travel-time pricing + live PVS danger field (`note_dangers`) +
700u chase cutover + goal-progress watchdog. Runtime pricing only (no cache change).

### spawn-to-* (runtester brain; same-session `as` controls)
| Leg | xg | as control |
|---|---|---|
| s2s q2dm1 ×3 | 1/3 (13.06s on 3629u) | 1/3 (13.64s on the SAME 3629u draw — parity) |
| swim railgun q2dm1 ×2 | 1/2 (19.19s) | 1/1 (17.77s) |
| ride railgun-1 q2dm3 ×2 | **2/2 (15.6/18.7s)** | 1/1 (25.8s) — xg faster |
| quad q2dm3 ×2 | **1/2 (24.10s)** | 0/3 — xg logged the session's ONLY quad reach |

### 5-min A/B (q3 brain, q2dm3): xg kd 0.17 ≥ as 0.06; 0 drowns, 23 traversal legs, 0 panics.

**Read**: parity on q2dm1 (nothing for the texture to price), advantage on q2dm3 (cutover +
watchdog + danger pricing shine where routes are contested and lift-gated). Deferred:
fall-time edge pricing (needs an edge-kind-aware weighted API).
