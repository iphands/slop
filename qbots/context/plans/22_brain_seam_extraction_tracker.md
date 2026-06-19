# Brain Seam Extraction — Tracker

## Overview
- Status: 70% complete (T0–T2 done; T3 verification pending, T4 scenario optional)
- Start date: 2026-06-18
- Deliverable: a single `brain::Brain` owning the decision sub-drivers; `bot_task` reduced to
  thin orchestration; byte-identical bot behavior (Plan 10 SUMMARY-line parity).

## Resume Instructions
Extract the decision/steering body of `bot_task` (`crates/qbots/src/main.rs:818–1111`) into
`brain::Brain::tick(view, nav: &mut dyn Navigator, cm, dt, ticks) -> BrainOutput`. Brain owns
combat/fsm/danger/steering/recovery/skill/roam; nav is injected per tick (uses, never modifies
the `Navigator` trait). Keep `heatmap_obs`/stats/`conn` in main; Brain exposes only
`heatmap_weights()` + `on_kill`/`on_death` hooks. The lift is **mechanical / verbatim** — any
SUMMARY-line drift in T4 means a logic edit slipped in. Commit at each task (`task(TN): …`);
run fmt + `clippy -D warnings` + test before every commit.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 0 | T0: plan + tracker + SERIES | `context/plans/22_*`, `SERIES.md` | done | |
| 1 | T1: Brain module + lifted `tick` body | `brain/src/brain.rs`, `lib.rs` | done | struct + ctor + `set_map` + hooks + verbatim tick; skeleton+body together (split warns on unused fields); 2 unit tests |
| 2 | T2: thin `bot_task` onto Brain | `qbots/src/main.rs` | done | construct Brain early, `set_map` at map load; `brain.tick` + hooks + `behavior()` log; removed fsm/combat/danger/skill/steering/recovery/roam_*/nav_graph locals; clippy clean, 104 brain + full workspace tests green |
| 3 | T3: zero-behavior verification | tracker, `SERIES.md` | pending | before/after SUMMARY parity; needs live server; move to completed/ |
| 4 | T4: migrate scenario.rs (optional) | `qbots/src/scenario.rs` | pending | combat-off, pinned-goal; or defer to follow-up |

## Baseline (pre-refactor SUMMARY lines — fill in T4)
| Scenario | reached | elapsed | flags (B/W/H/A/R) | exit |
|----------|---------|---------|-------------------|------|
| spawn-to-spawn q2dm1 | TBD | TBD | TBD | TBD |
| spawn-to-weapon rocketlauncher q2dm1 | TBD | TBD | TBD | TBD |

## After-refactor (fill in T4 — must match baseline)
| Scenario | reached | elapsed | flags (B/W/H/A/R) | exit |
|----------|---------|---------|-------------------|------|
| spawn-to-spawn q2dm1 | TBD | TBD | TBD | TBD |
| spawn-to-weapon rocketlauncher q2dm1 | TBD | TBD | TBD | TBD |
