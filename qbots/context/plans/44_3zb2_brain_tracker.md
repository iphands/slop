# Plan 44 — 3ZB2-Style Brain Implementation — Tracker

## Overview
- **Status**: 0% complete
- **Start date**: 2026-06-19
- **Goal**: Implement 3ZB2-style brain with route navigation, shortcuts, and state machine
- **Depends on**: Plan 43 (ride behavior), Plan 06 (brain plugin core)

## Resume Instructions
1. Read `context/plans/44_3zb2_brain.md` for full plan details
2. Read `context/distilled/brains/3zb2_brain.md` for 3ZB2 implementation guide
3. Start with T1 (extract route structure)
4. Follow RULES.md: commit after each task, fix all warnings before committing

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: Extract 3ZB2 Route Structure | `brain/src/brains/zb2.rs` | pending | Create RouteNode, RouteState, ZB2Brain skeleton |
| 2 | T2: Port Sequential Path Following | `brain/src/brains/zb2.rs` | pending | Implement route traversal + shortcut detection |
| 3 | T3: Port Linking Algorithm | `world/src/nav_generator.rs` | pending | Port G_FindRouteLink() with BSP-based LOS |
| 4 | T4: Implement State Machine | `brain/src/brains/zb2.rs` | pending | Port GRS_* states for elevators/doors/trains |
| 5 | T5: Integrate with Brain Plugin System | `brain/src/brains/mod.rs` | pending | Add ZB2 to BrainKind enum |
| 6 | T6: Competition Integration | `qbots/src/main.rs` | pending | Add --brain zb2 flag |
| 7 | T7: Performance Comparison | Tracker file | pending | Run competition vs Q3 brain, record results |

## Key Decisions

- **Approach**: Port 3ZB2's battle-tested navigation (not build from scratch)
- **Prioritization**: Focus on 1-2 brains (3ZB2 + Keys architecture), not 6
- **Dependencies**: Ride behavior (Plan 43) must be complete first

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Route generation from BSP unclear | Use existing nav graph, convert to route format |
| Entity detection for elevators | Reuse Plan 43's entity tracking |
| Shortcut optimization effectiveness | Test with q2dm1, measure improvement |
| State machine integration | Reuse Plan 43's ride logic |

## Notes

**Reviewer Feedback Summary**:
- neckbeard: "6 brains is too many. Start with 3ZB2 (essential state machine), use Keys for architecture reference."
- hoodie: "Fix Plan 10's movement bugs first. Define Brain trait, THEN add alternatives."

**What NOT to build**:
- ❌ Eraser-style dynamic learning (v2 feature)
- ❌ ACE's adjacency matrix (too complex)
- ❌ CRBot's BFS (suboptimal)
- ❌ Gladiator's AAS (already covered)

---

**Related Plans**:
- Plan 43: Moving-platform ride behavior (prerequisite)
- Plan 37: Q3 brain (for comparison)
- Plan 06: Brain plugin core (foundation)
