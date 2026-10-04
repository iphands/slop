# qctrl Plan Series

This document tracks the dependency chain and status of all plans for the qctrl project.

**Next free plan number: `15`.**

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

## Completed Plans

Completed plans are moved to `context/plans/completed/`.

## Notes

- Plans are numbered sequentially
- Sub-plans use `NN_N_name` format (e.g., `02_1_map_scanner`)
- Trackers pair with each plan: `NN_name_tracker.md`
