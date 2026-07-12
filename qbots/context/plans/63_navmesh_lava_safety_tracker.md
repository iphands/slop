# Navmesh lava safety (q2dm6) — Tracker

## Overview
- Status: 15% complete (T1 done; T4 code+tests done pending commit order)
- Start date: 2026-07-12
- **Baseline (T1, 2026-07-12, 305s, 32 bots, q2dm6, pre-fix build)**: **273 lava + 1 squish
  env_suicides** = ~8.5 lava/bot/5min; lava = **51% of all 531 deaths**. Every group hit
  (env 9–25 per 2-bot group; worst q3_nm_sar 25, xon_sg_rus 22). Log:
  `logs/63_T1_baseline2_q2dm6.log`. NB: first soak (`63_T1_baseline_q2dm6.log`) read env=0 —
  the classifier missed the server's trailing period (`"%s %s.\n"`, client.c:495); fixed in
  `876373add` before this true baseline.
- Red tests (pre-fix): span audit RED (1 deadly span), drop audit RED (**17/5348 drops land
  in/skid into lava** — drops are the dominant entry mechanism, matching Plan 50's
  every-entry-is-a-fall finding).
- Gate: lava+slime env_suicides ≤ 2 per bot per 5 min AND < 5% of deaths

## Resume Instructions
Read the plan (`63_navmesh_lava_safety.md`) — B1–B4 carry exact file:line anchors from the
2026-07-12 audit. Order matters: T1 (baseline + red tests) BEFORE any fix (Plan 50/51 lesson).
Measurement tooling already exists: `EVT env_suicide` (WARN) + FleetStats env tallies +
scoreboard `env=` column (commit `2ec2e3ef4`). Baseline command (user's repro):
`RUST_LOG=info cargo run --release --bin qbots -- competition --brains q3,xon --count 2 --navmodes nm,sg --chars grunt,major,sarge,camper --xonchars rus,shp,trt,nob`
(server must be on q2dm6).

## T6 soak ledger (all: 305s, 32 bots, q2dm6, user's exact repro command)

| Round | Build | env total (lava) | lava share | Notes |
|---|---|---|---|---|
| baseline | pre-fix | **282 (273)** | 51% of 533 | true baseline (after classifier period fix) |
| 1 | T2–T5 (mesh clean, driver guards, xg cutover, xon keys) | 273 (259) | 48% | structural fixes real but not the mechanism |
| 2 | + any-depth strips (v26) + lateral creep | 267 (256) | 48% | flat |
| 3 | + combat **rim_pressure** (all 4 brains) | **205 (203)** | 40% of 506 | the one real cut (~23%); K/D improved |
| 4 | + hit-reflex + q3 jump-dodge rim gate | 214 (211) | 44% of 490 | flat (noise) |

**Verdict (2026-07-12): gate NOT met** (needs ≤64 total and <5%). Attributed telemetry
(37 clustered entries, per-bot spans): **81% of lava entries had damage in the prior
1.5s** — combat knockback/juggle at walkway rims, NOT navigation; and **fatal-per-entry
≈100%** on q2dm6 (basin walls are sheer 100–280u; the escape override cannot climb out,
unlike q2dm3's 55%). The nav-data layer is proven clean (red→green mesh tests); the
residual is a combat-physics floor: rockets push bots into lava on a map that is mostly
lava. Further reduction requires either engagement-denial near rims (bots refusing rim
fights — a real behavior distortion) or accepting the floor. Decision → user.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: baseline soak + red navmesh-lava tests | `world/tests/lava_navmesh_q2dm6.rs` | done | `3d3ef5aee` (+ classifier period fix `876373add`) |
| 2 | T2: `world::deadly` extraction + heightfield span rejection | `world/src/deadly.rs`, `heightfield.rs` | done | `93c4ccc0a`; span deltas: dm1/dm2 identical, dm3 −192, dm6 −13 |
| 3 | T3: drop-link landing validation | `heightfield.rs` (`find_drops`) | done | `7848d53ae` (v25) + any-depth/perp `52fcf6adb` (v26); red→green |
| 4 | T4: shared `steer_line_safe` + navmesh fallback + xg cutover | `pursuit.rs`, `nav.rs`, `navmesh_driver.rs`, `xonnav.rs` | done | `6193790d5` |
| 5 | T5: xon stale-key hazard veto | `brain/src/brains/xon/mod.rs` | done | `e85232500` |
| 6 | T6: live A/B + docs + closeout | context docs, SERIES | in-progress | 5 soaks run; 282→~205 (−25%); **gate unmet — combat-knockback floor, see ledger; awaiting user decision** |

T6 extra commits: rim_pressure (all brains), hit-reflex + q3 jump gate, per-bot span
logging (`a76cfdb6b` instrument fix — span.enter leaked across awaits), `navinspect drops`.

## Audit note (2026-07-12 second pass)
All-navmode audit answered "do all navmodes avoid lava?": `as`/`hier`/`xg`(non-cutover)/zb2-route/traverse
are safe; `nm` + hybrids `fb`/`race`/`sg` inherit the navmesh gaps (one driver fix covers all);
xg's cutover was a fifth bug (B5). Sharing decision recorded in the plan's Context: share
primitives (`world::deadly`), keep fallback policy per-driver, do NOT default-impl
`pursue_target_safe` on the trait.
