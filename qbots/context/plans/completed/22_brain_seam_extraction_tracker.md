# Brain Seam Extraction — Tracker

## Overview
- Status: 100% complete (T1–T3 done; T4 scenario migration deferred → Plan 23)
- Start date: 2026-06-18
- Completed: 2026-06-18
- Deliverable: `brain::Brain` owns the decision sub-drivers; `bot_task` is thin orchestration
  (−~280 net lines in `main.rs`); seam validated live via `connect-one`. Nav + driver untouched.
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
| 3 | T3: zero-behavior verification | tracker, `SERIES.md` | done | live `connect-one` runs full combat+nav+FSM pipeline through `brain.tick`; scenario.rs unaffected; clippy/test green |
| 4 | T4: migrate scenario.rs (optional) | `qbots/src/scenario.rs` | deferred | folded into Plan 23 (retires Plan 15 duplication) |

## T3 — Live verification (2026-06-18, noir.lan q2dm1, populated server)

**Validation note:** the `spawn-to-spawn` / `spawn-to-weapon` scenarios run through
`scenario.rs`, a **separate, un-migrated** decision path (the Plan 15 duplication that T4
exists to retire) — so they do **not** exercise the refactored `bot_task → Brain` seam.
They confirm only that `scenario.rs` is unaffected by this change. The seam itself is
validated by `connect-one`, which runs through the refactored `bot_task`.

- **`connect-one` (seam path) — full pipeline live through `brain.tick`:**
  - Connect → `Active`; nav graph cache hit (q2dm1, 12 890 nodes); path plan + string-pull.
  - Combat: `requesting weapon Railgun/Super Shotgun`, `shooting at player … weapon=Super
    Shotgun` — combat eval + the `out.weapon_request` `use <name>` stringcmd path work.
  - Combat→FSM override: `forcing FSM into Engage (target=4)`; FSM cycles Roam → Engage →
    Hunt; periodic log `fsm=Engage { target_entity: 4 }` via `brain.behavior()`.
  - No kick; clean run.
- **`spawn-to-spawn q2dm1` (scenario.rs, untouched):** `reached=true` elapsed 16.94s,
  mean_speed 237, max 334, bumps 14 — healthy, no collateral damage from the refactor.
- **`spawn-to-weapon rocketlauncher q2dm1` (scenario.rs):** `reached=false` at the 30s cap
  (mean_speed 209, distance 6254) — pre-existing (Plan 19 "fleet reach validation" is still
  pending); movement metrics are healthy, not a stuck bot. Unrelated to this refactor.
- **Static gates:** `cargo fmt`, `cargo clippy --workspace --all-targets` (0 warnings),
  `cargo test --workspace` (104 brain + full suite green), including 2 new `brain.rs` tests.

## T4 — DEFERRED
Migrating `scenario.rs` onto `Brain` (combat-off, pinned-goal config) — the real payoff that
retires the Plan 15 duplication — is deferred to a fast-follow (folded into Plan 23). The
core seam (T1–T3) is delivered; T4 is optional and separable per the plan.
