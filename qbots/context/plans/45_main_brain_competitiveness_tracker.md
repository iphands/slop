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
| 3 | fast fire cadence + kite hp-only | — | — | — | — | — |

## Key finding
Main already has **perfect aim** (skill-5 clamps accuracy rating to 5.0, zero jitter). The gap is **not aim** — it's **fire latency + deaths**:
- Main waits ~0.9s (weapon-switch lockout) + ~0.4s (reaction reset on target acquire) ≈ **1.3s before its first shot**; q3 fires ~0.3s after sighting. **q3 shoots first, always.**
- Kiting/fleeing cuts deaths but also cuts kills ~1:1 → net kd flat. The lever is **kill faster**, not fight less.

## Task status
- [x] T1 underpowered disengage (main.rs) — reuses `q3char::bot_aggression`, fighting retreat.
- [x] T2 weighted item picker (`items::best_item_goal_weighted`, main-only).
- [ ] T3 flee tuning + dodge; iterate to a consistent win.

## Notes
- Baseline diagnosis: `main` dies ~2× as often as `q3` (38 vs 19). Kills are competitive
  (18 vs 26). Deaths are the lever → disengage-when-outgunned is the primary fix.
- Constraint honored: no edits under `crates/brain/src/brains/q3/`.
