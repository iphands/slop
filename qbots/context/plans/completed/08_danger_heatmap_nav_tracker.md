# Danger/Popularity Heatmap Nav Overlay — Tracker

## Overview
- Status: 100% complete (code + deterministic verification)
- Start date: 2026-06-14
- Plan: `08_danger_heatmap_nav.md`
- Depends on: Plan 05 (world: nav graph + trace), Plan 06 (brain: perception/events)
- Exit criterion: bots visibly detour around a known death-trap within ~30 s, gravitate toward hot lanes,
  and the heatmap decays so quieted nodes stop detouring.

## Resume Instructions
1. This is the **novel** plan (not a port) — Eraser can't do this (its graph is static topology). Motivation:
   `distilled/eraser.md §10 & §13-D`.
2. **Per-bot overlay, never shared** — AGENTS.md forbids shared mutable world state across bot tasks. Each bot's
   heatmap reflects its own PVS-limited observations.
3. Read-only topology: never mutate the `Arc<World>` nav graph — overlay is a side-table keyed by node id.
4. This is **strategic** routing (minute-scale); it composes with Plan 07 T3's **tactical** projectile dodge
   (frame-scale). Don't let them fight — strategic sets path/goal, tactical overrides a frame.
5. Keep updates **budgeted** (Eraser `optimize_marker`-style rotating cursor) so we never stall a tick.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: event ingestion | `brain/src/observed.rs` | done | obituary→victim last-known node (word-boundary match); presence from enemy deltas; self death/damage via health; conn surfaces svc_print |
| 2 | T2: heatmap + decay | `brain/src/heatmap.rs` | done | exp danger decay, EMA popularity (+ independent TAU_POP decay), budgeted |
| 3 | T3: risk-weighted A\* | `world/src/navgraph.rs`, `brain/src/nav.rs` | done | `NavGraph::path_weighted` adds per-src-node overlay (clamp EPS); `NavigationDriver` carries the overlay + desperate unweighted re-query (degeneracy guard 5× straight-line) |
| 4 | T4: PVS-honest attribution | `brain/src/observed.rs` | done | only observed/recent players attributed (PLAYER_NODE_TTL guard); HeatmapSnapshot + hot_nodes/total_danger for periodic debug logging (wired in T5) |
| 5 | T5: tune + integrate | `brain/src/skill.rs`, `qbots/src/main.rs` | done | `BotSkill::heatmap_weights` (skill→W_danger, personality→W_pop); observer created at nav-load, fed each tick (self death/damage via health, enemy presence, obituary prints via drain_prints), overlay refreshed before nav plans; death forces replan; periodic snapshot log; composes with 07 T3 tactical dodge by construction |
| 6 | T6: verify | `brain/tests/heatmap_pipeline.rs`, `context/distilled.md` | done | deterministic integration test proves detour→decay-restore + skill-scaling; gravitation proven at pathfinding unit level; **live-server confirmation deferred — `noir.lan:27910` down 2026-06-15** (snapshot debug log wired to confirm when it's back) |
