# qctrl Plan Series

This document tracks the dependency chain and status of all plans for the qctrl project.

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
| 01 | Project Setup & Config | `pending` | N/A |
| 02 | Map Listing API | `pending` | 01 |
| 03 | Frontend Scaffolding | `pending` | 01 |
| 04 | Deathmatch Controls UI | `pending` | 03 |
| 05 | Map Selection UI | `pending` | 02, 03 |
| 06 | Player Management | `pending` | 02, 07 |
| 07 | Log Streaming | `pending` | 01, 03 |
| 08 | Status Dashboard | `pending` | 04, 05, 06, 07 |
| 09 | Settings Persistence | `pending` | 01 |
| 10 | Final Testing & Polish | `pending` | 01-09 |
| 11 | Deployment Setup | `pending` | 10 |

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
