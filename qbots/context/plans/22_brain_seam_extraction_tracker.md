# Brain Seam Extraction — Tracker

## Overview
- Status: 10% complete
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
| 0 | T0: plan + tracker + SERIES | `context/plans/22_*`, `SERIES.md` | in-progress | this file |
| 1 | T1: Brain skeleton | `brain/src/brain.rs`, `lib.rs` | pending | types + hooks, no logic moved |
| 2 | T2: lift body into `Brain::tick` | `brain/src/brain.rs`, `main.rs` | pending | verbatim; cfg guards for combat-off/goal-override |
| 3 | T3: thin `bot_task` | `qbots/src/main.rs` | pending | brain.tick + move_ctrl + hooks; delete dead locals |
| 4 | T4: zero-behavior verification | tracker, `SERIES.md` | pending | before/after SUMMARY parity; move to completed/ |
| 5 | T5: migrate scenario.rs (optional) | `qbots/src/scenario.rs` | pending | combat-off, pinned-goal; or defer to follow-up |

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
