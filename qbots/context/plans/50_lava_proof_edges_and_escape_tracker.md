# Lava-Proof Base Edges + In-Lava Escape — Tracker

## Overview
- Status: 0% complete
- Start date: 2026-07-10
- Trigger: Plan 48 post-fix soak — 360 lava-burn frames in 5 min; bots burn 15+ s to death (soak log: session scratchpad `soak_1.log`)

## Resume Instructions
Read Plan 50 Context (soak evidence + E1/E2). T1 (world) and T2 (brain) are independent;
T3 needs the live server (`noir.lan:27910`, q2dm3) and the baseline soak metrics recorded in
the plan.

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: floor-validate flat edges + stair deadly check, cache v22 | `world/navgraph.rs`, `world/mapcache.rs` | done | all 8 q2dm caches regen clean; ride/water reachability green |
| 2 | T2: escape_from_lava override in all brains | `brain/hazard.rs`, `brains/*` | done | `EVT lava_escape` counter; pak-gated escape test |
| 3 | T3: post-fix soak comparison; close plan (+P49 T2) | `context/brain_notes.md` | pending | |
