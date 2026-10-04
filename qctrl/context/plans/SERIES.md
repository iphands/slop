# qctrl Plan Series

This document tracks the dependency chain and status of all plans for the qctrl project.
**Read this before creating a plan. Update it whenever a plan is added, starts, completes, or is
abandoned** — in the same commit as that change.

Plan status: `pending` | `in-progress` | `blocked` | `done` | `abandoned`.
Plans further out are intentionally light: an untouched pending plan is a **hypothesis, not a
contract** — re-read and revise it before starting (RULES.md Rule C).

**Next free plan number: `15`.**

> **North star (user directive, 2026-10-04):** qctrl keeps an unattended Q2 server healthy:
> rotation, guards and status run headless; the UI is a convenience, not a dependency. <!-- hp:north-star -->
>
> **Ordering principle:** Safety of the live server first (guards, sync), then headless behavior,
> then UI.

## Currently Active <!-- hp:active -->

<!-- One line per in-progress plan, in the order they should be worked:
     - **Plan NN** — <title>: in progress, X/Y tasks done; next TN (<short title>).
     When nothing is in progress: "None in progress — next up: Plan NN (`/hands-plan resume NN`)." -->

- **Plan 14** — hands-plan v1 Migration: in progress, 5/7 tasks done; next T6 (root `AGENTS.md`
  block, de-dup, `CLAUDE.md` link).

## Plan Dependencies

```
01_project_setup (Foundation)
    ├── 02_map_listing
    │   └── 05_map_selection
    ├── 03_frontend_scaffolding
    │   ├── 04_deathmatch_controls
    │   │   └── 08_status_dashboard
    │   ├── 05_map_selection
    │   ├── 06_player_management
    │   └── 08_status_dashboard
    ├── 07_log_streaming
    │   └── 08_status_dashboard
    ├── 09_settings_persistence
    └── 10_final_testing
        └── 11_deployment
```

## Plan Status

| # | Plan | Status | Depends On |
|---|------|--------|------------|
| 01 | Project Setup & Config | `done` | N/A |
| 02 | Map Listing API | `done` | 01 |
| 03 | Frontend Scaffolding | `done` | 01 |
| 04 | Deathmatch Controls UI | `done` | 03 |
| 05 | Map Selection UI | `done` | 02, 03 |
| 06 | Player Management | `done` | 02, 07 |
| 07 | Log Streaming | `done` | 01, 03 |
| 08 | Status Dashboard | `done` | 04, 05, 06, 07 |
| 09 | Settings Persistence | `done` | 01 |
| 10 | Final Testing & Polish | `done` | 01-09 |
| 11 | Deployment Setup | `done` | 10 |
| 12 | sv_maplist Resilience + Empty-Map Guards | `done` — closed 2026-10-04; verified live on noir (drift repaired in ≤60 s, guards return 400, fraglimit end rotates via `sv_maplist`) | 11 |
| 14 | hands-plan v1 Migration | `in-progress` — migrate RULES, SERIES, skeletons, `context/AGENTS.md` and root `AGENTS.md` to hands-plan v1 (2026-10-04 resync checklist) | N/A |

## Post-1.0 Plans

Plans 01–11 shipped the production qctrl. Plan 12 (2026-07-12) hardens the server
against the `maps/.bsp` crash: continuous `sv_maplist` re-sync + empty-map guards
at the API and frontend layers. Incident forensics live in the qbots repo
(`../qbots/context/plans/64_map_change_survival_tracker.md`).

Plan 12 closed 2026-10-04 with two open follow-ups, kept in its tracker rather than
numbered here: an unexplained startup `sv_maplist` miss (`push_sv_maplist` never
reads the value back), and `e2e-test.js` mutating the live server from inside
`testall`. See `completed/12_sv_maplist_resilience_tracker.md` → Follow-ups.

## Execution Order

1. **Phase 1 (Foundation)**: Plan 01
2. **Phase 2 (Backend)**: Plan 02, 07, 09
3. **Phase 3 (Frontend)**: Plan 03, 04, 05, 06
4. **Phase 4 (Integration)**: Plan 08
5. **Phase 5 (QA)**: Plan 10
6. **Phase 6 (Release)**: Plan 11

## Abandoned / Superseded <!-- hp:abandoned -->

| Plan | Reason | Superseded by | Date |
|------|--------|---------------|------|

*(Record the reason. A plan dropped without one gets re-attempted six months later by someone who
doesn't know it failed.)*

## Notes

- Plans are numbered sequentially
- Sub-plans use `NN_N_name` format (e.g., `02_1_map_scanner`)
- Trackers pair with each plan: `NN_name_tracker.md`

---

Completed plans move to `completed/`, abandoned ones to `abandoned/` (RULES.md Rule C). When this
file passes ~30 KB, move the narrative of fully closed series to `completed/SERIES_ARCHIVE.md` and
leave one summary row per series here.
