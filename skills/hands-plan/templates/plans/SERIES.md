<!-- hands-plan:v1 {{DATE}} -->
# {{PROJECT_NAME}} — Plan Series

Dependency chain and status of every plan. **Read this before creating a plan. Update it whenever
a plan is added, starts, completes, or is abandoned** — in the same commit as that change.

Plan status: `pending` | `in-progress` | `blocked` | `done` | `abandoned`.
Plans further out are intentionally light: an untouched pending plan is a **hypothesis, not a
contract** — re-read and revise it before starting (RULES.md {{R_LIFECYCLE}}).

**Next free plan number: `{{NEXT_FREE}}`.** <!-- hp:next-free -->

> **North star (user directive, {{DATE}}):** {{NORTH_STAR}} <!-- hp:north-star -->
>
> **Ordering principle:** {{ORDERING_PRINCIPLE}}

## Currently Active <!-- hp:active -->

{{ACTIVE}}

## Plans <!-- hp:plans -->

Keep **Notes** to ~25 words — the detail lives in the plan's tracker.

| Plan | Title | Depends on | Status | Notes |
|------|-------|-----------|--------|-------|
{{PLAN_ROWS}}

<!-- Add these sections when they earn their place:

## Dependency Chain
```text
01_foundation
    ├── 02_feature_a
    │     └── 04_feature_c
    └── 03_feature_b
```

## Milestones
- After **01**: <an observable capability, phrased so it can be checked>.

## Execution Order
1. <which plans, in what order, and what may run in parallel>

## Backlog Rationale
- **NN — Title.** Why it exists and what it is blocked on, so whoever picks it up starts with context.
-->

## Abandoned / Superseded <!-- hp:abandoned -->

| Plan | Reason | Superseded by | Date |
|------|--------|---------------|------|

*(Record the reason. A plan dropped without one gets re-attempted six months later by someone who
doesn't know it failed.)*

## Standing Constraints <!-- hp:constraints -->

{{CONSTRAINTS}}

---

Completed plans move to `completed/`, abandoned ones to `abandoned/` (RULES.md {{R_LIFECYCLE}}). When this
file passes ~30 KB, move the narrative of fully closed series to `completed/SERIES_ARCHIVE.md` and
leave one summary row per series here.
