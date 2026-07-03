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
- [x] T1 disengage/kite when underpowered (main.rs) — evolved into loadout-gated kite +
  hard-flee-to-weapon (the raw `bot_aggression<50` trigger was too twitchy; see iters 1–3).
- [x] T2 weighted item picker (`items::best_item_goal_weighted`, main-only).
- [x] T3 flee tuning + dodge — fast strafe juke (STRAFE_PERIOD 3.0→0.6s) + fire-cadence fix.

## Outcome (stopped by user decision, 2026-07-03)
**Result: main kd 0.47 → 0.68 (~+45%), deaths 38 → 25.** Not a win over q3-major (~1.3).

Shipped changes (all q3-untouched; two commits):
1. Fire cadence: switch-lockout 0.9→0.2s, reaction base halved (main+sentry `combat.rs`).
2. Weapon-rush: Blaster → fight evasively + disengage to grab a real gun; hitscan → stand & delete.
3. Weighted item picker (weapon hunger when weak, health/armor hunger when hurt).
4. Fast combat strafe juke (0.6s flip; main-only).

**Why we stopped:** main already has perfect hitscan aim and a *faster* reaction than q3
(0.20 vs 0.30s), yet still loses per-engagement — the gap is raw combat quality, not tactics.
Every defensive/positioning variant plateaus at ~0.5–0.68 (flee/kite/hold-range/jink all cut
kills ~1:1 with deaths). Closing the rest needs a real combat-strength edge (a different kind
of change than the requested strategy work); user chose to keep the ~45% gain and stop here.

### Reverted experiments (measured worse or neutral — do not re-try blindly)
- Combat jink (jump during fights): kills tanked — a hitscan railer leads the predictable hop.
- Hold long range (320u): neutral; cut kills as much as deaths.
- 360° acquire FOV: no help (main's 90° param is already a 180° cone; LOS still gates).
- Randomized strafe leg [0.3,0.9]s: worse — short legs vibrate in place (net-zero dodge). Fixed
  0.6s is better.
- Full run-to-item retreat (aggression<50): passive, back-turned deaths — worst of all.

## Notes
- Baseline diagnosis: `main` dies ~2× as often as `q3` (38 vs 19). Kills are competitive
  (18 vs 26). Deaths are the lever → disengage-when-outgunned is the primary fix.
- Constraint honored: no edits under `crates/brain/src/brains/q3/`.
