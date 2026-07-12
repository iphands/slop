# Xonotic character + core primitives — Tracker

## Overview
- Status: 100% complete (closed 2026-07-11)
- Start date: 2026-07-11
- Deliverable: pure unit-tested Xonotic primitives (`xonchar`, `xoncore::{rating,aim,keyboard}`, `NavGraph::flood_costs`) — additive only, no brain, no CLI.
- Blocks: Plan 60 (xon brain), Plan 61 (xon navmode uses flood/rating pieces).

## Resume Instructions
1. Read `context/plans/RULES.md`, then Plan 59, then `context/distilled/xonotic.md` §2, §4, §5, §7 (the authoritative research — vendor file:line cites live there).
2. Vendor ground truth: `vendor/xonotic/data/xonotic-data.pk3dir/qcsrc/server/bot/default/` (sparse clone, qcsrc only — see distilled header to re-fetch).
3. Mirror the Plan 36 (`q3char`) pattern: pure functions/structs, synthetic inputs, seeded RNG, pinned vendor constants.
4. HARD RULE: no edits to existing brains — additive modules + exports only.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: XonSkill + presets | `crates/brain/src/xonchar.rs` | done | `6e328a2d4` — 12 axes, 4 presets (rus/shp/trt/nob), Q2 weapon base-value table |
| 2 | T2: rating module | `crates/brain/src/xoncore/rating.rs` | done | `3d8529a84` — + Lcg/wrap180; ammo formula corrected vs distillation (gives/max(0.5,have)) |
| 3 | T3: XonAim | `crates/brain/src/xoncore/aim.rs` | done | `31006aa20` — full aimdir/bot_aim port; vendor int-truncation regression documented+corrected; ballistic lead deferred to Plan 60 T4 |
| 4 | T4: keyboard quantizer | `crates/brain/src/xoncore/keyboard.rs` | done | `d7e122ee4` — vocabulary tiers + rekey cadence + blend |
| 5 | T5: flood_costs | `crates/world/src/navgraph.rs` | done | `b6f0287ca` — flood_costs(_weighted), overlay parity w/ path_weighted, no cache bump |
| 6 | T6: docs + brain_notes + close | `context/brain_notes.md` | done | brain_notes dated entry; distilled §2 ammo fix; plan → completed/ |

## Verification

- [x] All new modules unit-tested with pinned vendor constants (seeded RNG where randomized) — 27 tests
- [x] `flood_costs` overlay/route parity vs `path_weighted` on synthetic graphs
- [x] Existing brains byte-untouched (additive modules + exports only)
- [x] Zero warnings, clippy clean, fmt, tests green at every commit (Rule A/B)
