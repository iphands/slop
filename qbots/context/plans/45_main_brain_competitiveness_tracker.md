# Plan 45 Tracker — `main` brain competitiveness vs `q3`

> Paired with `45_main_brain_competitiveness.md`.

## Test command
`timeout 300 cargo run --release --bin qbots -- competition --count 4 --brains q3,main --chars major --navmodes hybrid-fallback`

## Results log (5-min runs, FINAL scoreboard)

| Iter | Change | main kd | main K/D | q3 kd | q3 K/D | main wins? |
|------|--------|---------|----------|-------|--------|-----------|
| base | (unchanged main) | 0.47 | 18/38 | 1.37 | 26/19 | no |
| 1 | full disengage (aggression<50) + weighted items | 0.47 | 18/38 | 1.37 | 26/19 | no (passive; back-turned deaths) |
| 2 | kite (weak\|hp<60) + weighted items | 0.39 | 9/23 | 1.59 | 27/17 | no (over-kite → kills halved) |
| 3 | fast fire cadence + kite hp-only | 0.58 | 19/33 | 1.21 | 23/19 | no (kills up, deaths still 33) |
| 4 | + combat jink (jump) | — | — | — | — | reverted (kills tanked; hitscan leads the hop) |
| 5 | + hold long range (320u) | — | — | — | — | reverted (neutral; cut kills too) |
| 6 | + 360° acquire FOV | — | — | — | — | reverted (no help) |
| 7 | weapon-rush (blaster→grab gun/evade) | ~0.5 | 17/33 | ~1.4 | — | no (even early, q3 surged late) |
| 8 | + fast strafe juke (3.0s→0.6s flip) | 0.68 | 17/25 | 1.32 | 25/19 | no (best yet; deaths 38→25) |

## Key findings (from obituary analysis)
- Main already has **perfect hitscan aim** (skill-5 clamps accuracy to 5.0) and its reaction
  (0.20s) is now **faster than q3's** (0.30s) — yet it lost. So the gap is **damage exposure**,
  not aim.
- **Fire latency** was real: old 0.9s switch-lockout + 0.4s reaction ≈ 1.3s before first shot;
  q3 fired at ~0.3s. Fixed in iter3 (0.2s lockout, 0.20s reaction).
- **Losing loop:** main dies → respawns with only the Blaster → loses the projectile duel to
  q3 → dies again. Fix: weapon-rush (evade + grab a real gun on the Blaster) — iter7.
- **Smoking gun:** main's combat strafe flipped only every **3.0s** — a straight line q3 could
  trivially lead. Cut to 0.6s (iter8) → deaths 38→25, kd 0.68.
- Defensive-only tuning (flee/kite/hold-range) plateaus ~0.5: it cuts kills and deaths ~1:1.
  Progress came from **shooting sooner + being harder to hit + arming up**, not fighting less.

## Task status
- [x] T1 underpowered disengage (main.rs) — reuses `q3char::bot_aggression`, fighting retreat.
- [x] T2 weighted item picker (`items::best_item_goal_weighted`, main-only).
- [ ] T3 flee tuning + dodge; iterate to a consistent win.

## Notes
- Baseline diagnosis: `main` dies ~2× as often as `q3` (38 vs 19). Kills are competitive
  (18 vs 26). Deaths are the lever → disengage-when-outgunned is the primary fix.
- Constraint honored: no edits under `crates/brain/src/brains/q3/`.
