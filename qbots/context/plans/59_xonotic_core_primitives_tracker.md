# Xonotic character + core primitives — Tracker

## Overview
- Status: 0% complete
- Start date: —
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
| 1 | T1: XonSkill + presets | `crates/brain/src/xonchar.rs` | pending | |
| 2 | T2: rating module | `crates/brain/src/xoncore/rating.rs` | pending | enemy hp param, default 100 |
| 3 | T3: XonAim | `crates/brain/src/xoncore/aim.rs` | pending | ballistic lead deferred to Plan 60 |
| 4 | T4: keyboard quantizer | `crates/brain/src/xoncore/keyboard.rs` | pending | |
| 5 | T5: flood_costs | `crates/world/src/navgraph.rs` | pending | NO cache VERSION bump |
| 6 | T6: docs + brain_notes + close | `context/brain_notes.md` | pending | git mv to completed/ |

## Verification

- [ ] All new modules unit-tested with pinned vendor constants (seeded RNG where randomized)
- [ ] `flood_costs` ≈ `path` cost parity on synthetic graphs
- [ ] Existing brains byte-untouched
- [ ] Zero warnings, clippy clean, fmt, tests green at every commit (Rule A/B)
