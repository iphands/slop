# Bot performance findings — competition runs

> Running log of notable competition results + what they mean. Read alongside
> `acceptance.md` (the variance rules live there: **never** conclude from one run;
> the findings below marked *replicated* are the ones internally confirmed across
> both navmodes within the same run).

---

## 2026-07-13 — Run 2: 65-min rotation marathon, first board with hp/ap/wp

**Setup**: same 18-group matrix as Run 1 (below), 36 bots, **3926 s (~65 min)**, map change
every 10 min (~6–7 rotations — likely most of the q2dm* cycle vs Run 1's ~4 maps). First
marathon with the Plan 67/68 columns: `hp`/`ap` = health/armor **points** picked up, `wp` =
weapons **count**. Fleet totals: 4,126 kills / 5,713 deaths / 1,358 env suicides
(~1.05 kills/s); 229 deaths (4%) are untagged self-splash. All 36 bots alive at FINAL
through every rotation — durability boring again.

### Full board (K/D-ranked; cK/D = kills / (deaths − env))

| #  | group      | brain | nav | persona | kills | deaths | K/D  | env | env% | cK/D | hp  | ap  | wp |
|----|------------|-------|-----|---------|------:|-------:|-----:|----:|-----:|-----:|----:|----:|---:|
| 1  | mai_sg     | main  | sg  | —       | 277   | 278    | 1.00 | 86  | 31%  | 1.44 | 387 | 565 | 55 |
| 2  | q3_sg_cam  | q3    | sg  | Camper  | 231   | 240    | 0.96 | 80  | 33%  | 1.44 | 653 | 158 | 30 |
| 3  | mai_nm     | main  | nm  | —       | 258   | 276    | 0.93 | 76  | 28%  | 1.29 | 501 | 611 | 44 |
| 4  | xon_sg_shp | xon   | sg  | Sharp   | 301   | 352    | 0.86 | 68  | 19%  | 1.06 | 239 | 404 | 28 |
| 5  | q3_sg_sar  | q3    | sg  | Sarge   | 230   | 282    | 0.82 | 119 | 42%  | 1.41 | 646 | 523 | 48 |
| 6  | q3_sg_maj  | q3    | sg  | Major   | 220   | 275    | 0.80 | 103 | 37%  | 1.28 | 445 | 461 | 39 |
| 7  | q3_nm_cam  | q3    | nm  | Camper  | 195   | 248    | 0.79 | 79  | 32%  | 1.15 | 420 | 467 | 34 |
| 8  | xon_nm_shp | xon   | nm  | Sharp   | 295   | 379    | 0.78 | 44  | 12%  | 0.88 | 201 | 315 | 25 |
| 9  | q3_nm_gru  | q3    | nm  | Grunt   | 221   | 286    | 0.77 | 73  | 26%  | 1.04 | 467 | 269 | 40 |
| 10 | xon_nm_trt | xon   | nm  | Turtle  | 269   | 372    | 0.72 | 48  | 13%  | 0.83 | 176 | 371 | 29 |
| 11 | q3_nm_sar  | q3    | nm  | Sarge   | 195   | 270    | 0.72 | 104 | 39%  | 1.17 | 531 | 650 | 31 |
| 12 | xon_sg_trt | xon   | sg  | Turtle  | 253   | 360    | 0.70 | 53  | 15%  | 0.82 | 297 | 560 | 30 |
| 13 | q3_nm_maj  | q3    | nm  | Major   | 181   | 260    | 0.70 | 88  | 34%  | 1.05 | 542 | 282 | 34 |
| 14 | q3_sg_gru  | q3    | sg  | Grunt   | 181   | 272    | 0.67 | 68  | 25%  | 0.89 | 389 | 537 | 40 |
| 15 | xon_nm_rus | xon   | nm  | Rusher  | 244   | 400    | 0.61 | 64  | 16%  | 0.73 | 152 | 443 | 32 |
| 16 | xon_sg_rus | xon   | sg  | Rusher  | 219   | 380    | 0.58 | 79  | 21%  | 0.73 | 295 | 578 | 53 |
| 17 | xon_sg_nob | xon   | sg  | Noob    | 177   | 380    | 0.47 | 69  | 18%  | 0.57 | 401 | 539 | 35 |
| 18 | xon_nm_nob | xon   | nm  | Noob    | 179   | 403    | 0.44 | 57  | 14%  | 0.52 | 301 | 341 | 23 |

### Aggregates

**By brain** (mai 4 bots, q3/xon 16 each; per-bot columns normalize the roster sizes):

| brain | kills | deaths | K/D  | env  | env% | cK/D | env/bot/5min | hp/bot | ap/bot | wp/bot |
|-------|------:|-------:|-----:|-----:|-----:|-----:|-------------:|-------:|-------:|-------:|
| mai   | 535   | 554    | **0.97** | 162  | 29%  | **1.37** | 3.1 | 222 | **294** | **24.8** |
| q3    | 1654  | 2133   | 0.78 | 714  | **33%** | 1.17 | **3.4** | **256** | 209 | 18.5 |
| xon   | **1937** | 3026   | 0.64 | 482  | 16%  | 0.76 | 2.3 | 129 | 222 | 15.9 |

**By navmode, per brain**:

| brain | nm K/D | sg K/D | Run 1 said | verdict |
|-------|-------:|-------:|-----------|---------|
| mai   | 0.93 | **1.00** | nm 1.24 > sg 0.97 | **FLIPPED — Run 1's "navmesh boost" was noise/map-mix** |
| q3    | 0.74 | **0.81** | sg > nm (0.89/0.78) | replicated: q3 prefers hybrid-segment |
| xon   | 0.64 | 0.65 | nm ≥ sg (0.68/0.60) | a wash |

**By persona** (summed across navmodes):

| roster | this run | Run 1 | verdict |
|--------|----------|-------|---------|
| q3     | Camper 0.87 > Sarge 0.77 > Major 0.75 > Grunt 0.72 | same order | **exact replication** |
| xon    | Sharp 0.81 > Turtle 0.71 > Rusher 0.59 > Noob 0.46 | Sharp > Rus ≈ Trt > Noob | ends replicate; Turtle now clearly above Rusher (both modes) |

### The happenings

1. **main wins again — both its groups in the top 3 — but the field compressed.** Winner
   K/D fell 1.24 → 1.00 and the spread tightened (0.38–1.24 → 0.44–1.00): over a longer,
   fuller rotation the brains are closer than Run 1 suggested. And the navmode story
   inverted for main (sg 1.00 > nm 0.93) — the single Run-1 finding that did NOT survive
   replication. Treat main's navmode choice as a don't-care until an N≥5 A/B says otherwise.

2. **The K/D-first sort (Plan 67) earned its keep on its first marathon.** Under the old
   kills-ranked board this run would have read as a xon victory — `xon_sg_shp` (301 kills)
   and `xon_nm_shp` (295) out-killed everyone. K/D-ranked they sit #4/#8, because they also
   died 352/379 times. xon is the fleet's volume fighter: 47% of ALL kills, worst efficiency.

3. **The combat hierarchy is rock-stable; board positions move with the env tax.**
   Combat-only K/D by brain: mai 1.37 > q3 1.17 > xon 0.76 — vs Run 1's 1.35 / 1.23 / 0.72.
   Two marathons, different map mixes, near-identical numbers. Per-engagement quality is a
   settled ranking; what shuffles the board is who donates deaths to the map.

4. **The env tax went UP fleet-wide — 1,358 suicides, 24% of all deaths** (Run 1: 20%).
   Every brain now exceeds the Plan 63 rate gate (mai 3.1, q3 3.4, xon 2.3 per bot/5min vs
   gate ≤2), where Run 1 had only q3 over. mai's env% doubled (15/21% → 31/28%). The longer
   run plausibly spends more time on hazard-heavy maps (q2dm3/q2dm6 tier) — map-mix
   confounding either way, but the absolute burn says the multi-map hazard tail is the
   fleet's #1 shared loss. `q3_sg_sar` is the poster child: **42% of its deaths were
   environmental** (119!) yet its combat K/D (1.41) is top-4 — Sarge/Major (the aggressive
   q3 presets) consistently burn hottest in both runs.

5. **First pickup economics, and they match each brain's code.** q3 hoovers health
   (256 hp/bot — its Plan 30 hurt→health-seek at work; Camper alone banked 653 hp against a
   miserly 158 ap). mai leads armor (294 ap/bot) and weapons (24.8 wp/bot) — the winner is
   also the best-armed, best-armored group, consistent with its weighted item picker.
   xon barely touches health (129 hp/bot, lowest on every hp cell) yet posts the best kill
   volume — it fights instead of healing, and its 3,026 deaths are the bill. Kills per
   weapon picked: xon 7.6 vs mai 5.4 / q3 5.6 — xon squeezes more from fewer guns (or just
   respawns into fights faster). One resource read to test later: xon's low hp intake is a
   tunable (rating weights), and finding 3 says its combat is the real gap — but cheap
   survivability wouldn't hurt a brain that dies 3,000 times an hour.

6. **Personas keep behaving like personalities.** q3's order replicated exactly
   (Camper > Sarge > Major > Grunt); xon's skill axis replicated at the ends (Sharp best,
   Noob worst, both modes, both runs) and Turtle now clearly beats Rusher in both modes —
   Run 1's Turtle-vs-Rusher ambiguity resolved toward Turtle.

### Cross-run scorecard (what to trust after two marathons)

| finding | status |
|---------|--------|
| Combat hierarchy mai > q3 > xon (cK/D) | **replicated, near-identical values — trust it** |
| q3 prefers hybrid-segment | replicated (2/2) |
| q3/xon persona orderings | replicated (q3 exact; xon ends + Turtle>Rusher resolved) |
| q3 aggressive presets burn most env deaths | replicated (Sarge/Major worst both runs) |
| mai navmode preference | **did NOT replicate (nm +0.27 → sg +0.07) — noise/map-mix** |
| env tax level | worsened fleet-wide; map-mix-sensitive, needs per-map attribution |

Follow-ups (revised): **(a)** per-map env attribution — log the map name with `EVT
env_suicide` tallies so the next marathon says WHICH maps collect the 24% tax; **(b)** q3
hazard-avoidance parity (unchanged, still worth ~0.4 K/D); **(c)** xon aim tuning
(unchanged); **(d)** drop the mai navmode question — measured twice, opposite answers.

---

## 2026-07-13 — Run 1: 18-group brain×navmode×persona rotation marathon

**Setup**: 36 bots (2 per group), 2637 s (~44 min), **map change every 10 min** (~4–5 maps
from the rotation — a multi-map aggregate, not a single-map result). Groups =
{`mai`, `q3`, `xon`} × {`nm` navmesh, `sg` hybrid-segment} × persona roster
(q3: Sarge/Major/Camper/Grunt; xon: Sharp/Turtle/Rusher/Noob; mai: none).
Predates Plans 67/68, so no `hp/ap/wp` pickup columns; board re-ranked here by K/D
(the new default sort). 2,515 kills / 3,314 deaths / 661 env suicides fleet-wide
(~0.95 kills/s); the 138 deaths that are neither frags nor env are self-splash suicides
the obituary classifier doesn't tag as environmental.

**Durability note**: every group still had `bots=2` at FINAL after 4+ hard rotations —
zero fleet attrition, the Plan 64/65 map-change/watchdog work holding in a long run.

### Full board (K/D-ranked, kills tiebreak — the new default)

*cK/D = combat-only K/D = kills / (deaths − env). It answers "how does the group trade
when the map isn't killing it."*

| #  | group      | brain | nav | persona | kills | deaths | K/D  | env | env% | cK/D |
|----|------------|-------|-----|---------|------:|-------:|-----:|----:|-----:|-----:|
| 1  | mai_nm     | main  | nm  | —       | 201   | 162    | 1.24 | 24  | 15%  | 1.46 |
| 2  | q3_sg_cam  | q3    | sg  | Camper  | 133   | 125    | 1.06 | 49  | 39%  | 1.75 |
| 3  | q3_sg_sar  | q3    | sg  | Sarge   | 147   | 151    | 0.97 | 50  | 33%  | 1.46 |
| 4  | mai_sg     | main  | sg  | —       | 149   | 154    | 0.97 | 32  | 21%  | 1.22 |
| 5  | q3_sg_maj  | q3    | sg  | Major   | 135   | 150    | 0.90 | 62  | 41%  | 1.53 |
| 6  | xon_sg_shp | xon   | sg  | Sharp   | 180   | 203    | 0.89 | 30  | 15%  | 1.04 |
| 7  | xon_nm_shp | xon   | nm  | Sharp   | 184   | 208    | 0.88 | 19  | 9%   | 0.97 |
| 8  | q3_nm_cam  | q3    | nm  | Camper  | 122   | 141    | 0.87 | 39  | 28%  | 1.20 |
| 9  | q3_nm_sar  | q3    | nm  | Sarge   | 132   | 167    | 0.79 | 60  | 36%  | 1.23 |
| 10 | xon_nm_trt | xon   | nm  | Turtle  | 166   | 218    | 0.76 | 20  | 9%   | 0.84 |
| 11 | q3_nm_gru  | q3    | nm  | Grunt   | 130   | 171    | 0.76 | 35  | 20%  | 0.96 |
| 12 | q3_nm_maj  | q3    | nm  | Major   | 122   | 169    | 0.72 | 63  | 37%  | 1.15 |
| 13 | xon_nm_rus | xon   | nm  | Rusher  | 149   | 213    | 0.70 | 10  | 5%   | 0.73 |
| 14 | q3_sg_gru  | q3    | sg  | Grunt   | 123   | 177    | 0.69 | 47  | 27%  | 0.95 |
| 15 | xon_sg_rus | xon   | sg  | Rusher  | 139   | 218    | 0.64 | 30  | 14%  | 0.74 |
| 16 | xon_sg_trt | xon   | sg  | Turtle  | 114   | 220    | 0.52 | 35  | 16%  | 0.62 |
| 17 | xon_nm_nob | xon   | nm  | Noob    | 105   | 248    | 0.42 | 28  | 11%  | 0.48 |
| 18 | xon_sg_nob | xon   | sg  | Noob    | 84    | 219    | 0.38 | 28  | 13%  | 0.44 |

### Aggregates

**By brain** (16 bots each for q3/xon, 4 for mai):

| brain | kills | deaths | K/D  | env | env% | cK/D | env/bot/5min |
|-------|------:|-------:|-----:|----:|-----:|-----:|-------------:|
| mai   | 350   | 316    | **1.11** | 56  | 18%  | **1.35** | 1.6 |
| q3    | 1044  | 1251   | 0.83 | **405** | **32%** | 1.23 | **2.9** |
| xon   | 1121  | 1747   | 0.64 | 200 | 11%  | 0.72 | 1.4 |

**By navmode, per brain** (the effect is brain-specific, NOT universal):

| brain | nm K/D | sg K/D | read |
|-------|-------:|-------:|------|
| mai   | **1.24** | 0.97 | navmesh is a big win for main |
| xon   | 0.68 | 0.60 | mild navmesh edge |
| q3    | 0.78 | **0.89** | q3 actually prefers hybrid-segment |

Overall nm 0.77 vs sg 0.74 — a wash; never quote a fleet-wide navmode number without
the per-brain split.

**By persona** (summed across both navmodes):

| roster | order (K/D) |
|--------|-------------|
| q3     | Camper 0.96 > Sarge 0.88 > Major 0.81 > Grunt 0.73 |
| xon    | Sharp 0.89 > Rusher 0.67 ≈ Turtle 0.64 > Noob 0.41 |

### The happenings — what this run actually says

1. **`main` won the marathon, and navmesh is what put it on top.** `mai_nm` took #1 at
   1.24 — the only group above 1.1 — while the identical brain on hybrid-segment sat at
   0.97. main also leads the brain aggregate (1.11) *and* the combat-only aggregate
   (1.35), so this isn't just hazard discipline: post-thrash-fix main out-trades
   everything at fleet scale. (Historical arc: 0.36 SIGNAL-deficit → parity → now ahead;
   confirm with an N≥5 aggregator run before celebrating.)

2. **q3's board position is self-inflicted, not combat weakness.** q3 groups burned
   **405 env suicides = 32% of all their deaths** (avg ~50/group vs ~25 xon / ~28 mai),
   ~2.9/bot/5min — the only brain over the Plan 63 rate gate (≤2/bot/5min). Strip the
   env deaths and q3 is a *monster*: the top three combat-only K/Ds on the board are
   q3_sg groups (Camper 1.75, Major 1.53, Sarge 1.46). The q3 fix isn't aim or tactics,
   it's hazard avoidance — its aggression walks it into lava/voids the other brains'
   Plan 48/50 gates dodge. Highest-leverage single fix identified this run.

3. **xon genuinely loses fights.** Cleanest env discipline of the three (11%, Rusher at
   just 5%) — yet the bottom three slots are all xon, and its combat-only K/D (0.72) is
   the worst by a wide margin. xon's problem is per-engagement combat quality
   (aim/dodge/fire cadence), the opposite diagnosis from q3. Plan 62 found Sharp reaches
   q3's band because "aim was the gap" — that gap is still open for the other presets.

4. **Personas differentiate, and it replicates.** *(replicated across navmodes — the
   trustworthy kind of single-run finding)* Sharp is the best xon in both modes
   (0.89/0.88); Noob is dead-last in both (0.42/0.38) — the skill axis works end-to-end.
   q3's Camper tops its roster in both modes (patience = fewer bad fights AND its 1.75
   combat K/D is the board's best), Grunt trails in both. Turtle-vs-Rusher flips with
   navmode (Turtle 0.76 on nm, 0.52 on sg) — the one persona × navmode interaction worth
   a targeted A/B if we ever tune Turtle.

5. **All three brains blow past the env "share of deaths" gate.** The Plan 63 gate says
   env < 5% of deaths; this rotation saw 18% / 32% / 11% (mai/q3/xon). Plan 63's q2dm6
   analysis attributed the residual to combat knockback into sheer basins, and the user
   accepted that floor — but a 10-min-rotation number this high says the multi-map env
   tail (q2dm3 lava, q2dm6 basins, …) is a fleet-wide tax worth revisiting, starting with
   q3 (finding 2).

6. **The fleet itself is now boring (in the best way).** 36/36 bots alive through 4+
   map changes, 44 minutes, no attrition, ~0.95 kills/s sustained — Plans 64/65
   delivered; long unattended rotation runs are a valid measurement platform now.

### Caveats & follow-ups

- **Single run.** Per `acceptance.md`, K/D gaps from one run are noise until confirmed
  at N≥5 with `tools/acceptance` (control-spread noise floor). The persona orderings and
  the q3 env skew are internally replicated (both navmodes agree) and safe to act on;
  the exact mai-vs-q3 gap and the navmesh boost sizes are not.
- Follow-ups, in leverage order: **(a)** q3 hazard-avoidance parity with the Plan 48/50
  gates (worth ~0.4 K/D by the cK/D delta); **(b)** xon non-Sharp aim tuning (Plan 62
  showed the mechanism); **(c)** re-run this exact rotation at N≥5 with the new
  `hp/ap/wp` columns (Plans 67/68) to see whether main's win correlates with resource
  control.
