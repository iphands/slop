# Bot performance findings — competition runs

> Running log of notable competition results + what they mean. Read alongside
> `acceptance.md` (the variance rules live there: **never** conclude from one run;
> the findings below marked *replicated* are the ones internally confirmed across
> both navmodes within the same run).

---

## 2026-07-13 — 18-group brain×navmode×persona rotation marathon

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
