# Plans — Rules & Conventions

> Read this before writing any plan file or tracker file in `context/plans/`.

---

## Plan File Format

### Naming

- `NN_name.md` — two-digit zero-padded number, snake_case name (e.g. `65_modelview_skel_fix.md`)
- Sub-plans: `NN_N_name.md` (e.g. `15_1_worldmap_parser_terrain.md`)
- Trackers: `NN_name_tracker.md` — always paired with the plan
- `SERIES.md` — master dependency chain across all plans (no number)

### Metadata Block

Every plan file must open with a title and this metadata block:

```markdown
# Plan NN — [Title]

> **Status**: pending | in-progress | done
> **Created**: YYYY-MM-DD
> **Depends on**: Plan N | N/A
> **Goal**: One-sentence deliverable description.
> **Agent**: implementation agent (ralph-loop) | sub-agent | etc.

---
```

### Required Sections (in this order)

#### `## TL;DR`

```markdown
**What**: One sentence describing what is being done.

**Deliverables**:
1. Concrete output one
2. Concrete output two

**Estimated effort**: Small (2 h) | Small–Medium (half day) | Medium (1 day) | Large (3 days)
```

#### `## Context`

Background, rationale, prior findings, and decisions made. Use H3 subsections for complex plans:

- `### Pre-Identified Bug/Issue` — confirmed bugs documented before coding starts
- `### Why [Approach]` — justification for a design choice
- `### Key Facts` — research findings, format details

#### `## Step-by-Step Tasks`

One H3 per task, labeled `T1`, `T2`, etc.:

```markdown
### T1: [Task title]

**File**: `path/to/file.rs`

**What to do**: Detailed instructions.

**Before**:
```rust
// old code
```

**After**:
```rust
// corrected code
```
```

For large plans, group tasks into parallel waves with an explicit dependency matrix.

#### `## Critical Files`

| File | Change | Priority |
|------|--------|----------|
| `path/to/file.rs` | Description of change | P0 |

Priority values: `P0` = blocking, `P1` = important, `P2` = nice-to-have.

#### `## Open Questions / Risks`

Numbered list. Each point names the risk and suggests a mitigation.

#### `## Verification Checklist`

One checkbox per task, each a testable assertion:

```markdown
- [ ] T1: `cargo test` passes with ≥ 90% coverage on touched modules
- [ ] T2: `./bin/debug-image` confirms humanoid silhouette
```

---

## Tracker File Format

Every non-trivial plan gets a paired tracker: `NN_name_tracker.md`.

```markdown
# [Plan Title] — Tracker

## Overview
- Status: N% complete
- Start date: YYYY-MM-DD
- [Other plan-specific metrics]

## Resume Instructions
[How to pick up work if interrupted]

## Progress

| # | Task | File / Module | Status | Notes |
|---|------|---------------|--------|-------|
| 1 | T1: ... | `path/file.rs` | pending | |
```

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped`

---

## Per-Task Execution Rules

These rules apply to **every task** (T1, T2, …) during implementation. They are not optional.

### Rule A — Zero build errors and warnings

After completing each task:

1. Run `cargo build` — must exit 0 with **zero** errors and **zero** warnings.
2. Run `cargo clippy` — must exit 0 with **zero** warnings.
3. If any warnings remain, fix them before marking the task done.
4. **Never mark a task `done` while compiler warnings are outstanding.**

### Rule B — Commit at every task boundary (or more often) <!-- hp:commit -->

**YOU MUST COMMIT BEFORE MARKING ANY TASK COMPLETE.** If you haven't committed, you haven't
finished.

1. Commit at the end of **every** task. Smaller intermediate commits are welcome. **Never wait
   for the plan to finish.**
2. One task per commit — do not batch tasks unless they are truly inseparable.
3. Message format: `[qctrl][P<n>][T<n>][<topic>] <imperative summary>` — sub-project scope (slop
   is a monorepo), plan number (not zero-padded), task number, short area tag. Outside a plan:
   `[qctrl][<topic>] <summary>`. Plan bookkeeping replaces the task tag:
   `[qctrl][P<n>][plan] add <title>`, `[qctrl][P<n>][revise] …`,
   `[qctrl][P<n>][close] <title>: <outcome>`.
   - Example: `[qctrl][P12][T1][watchdog] add background sv_maplist drift re-sync loop`
   - Example: `[qctrl][P14][plan] add hands-plan v1 migration`
   Commits before 2026-10-04 use `task(TN): …` or conventional prefixes; history is never rewritten.
4. **Tracker row in the same commit.** The row update — and any `context/` update the task
   produced — lands with the work. Stage explicit paths: `git add <paths>`, never `git add -A`.
5. The full project gate (Rule A) passes before every commit.
6. A change in observable behavior updates `DEPLOYMENT.md` in the same commit.
7. **Never push** — the human pushes after review. **No co-author trailers** unless asked.
8. Every task in a plan carries its `**Commit**:` line, so the reminder is baked in.

#### Project addendum

NOTE: YOU MUST make sure that linting, auto formating and (in rust) cargo clippy is run before every commit
NOTE: Fix all warnings before each commit
NOTE: All unit tests must pass before each commit!

**MANDITORY** bake commit reminders into the plan TODO / Task lists!

### Rule B2 — Git history is append-only <!-- hp:append-only -->

**Never rewrite a commit — not even the one you just made.** Banned unless the human explicitly
asks, in that moment:

| Banned | Why |
|---|---|
| `git commit --amend` | replaces a commit that may already be public |
| `git rebase` (any form) | rewrites every commit it touches |
| `git reset --hard`, any reset that drops a commit | discards history |
| `git push --force` / `--force-with-lease` | forces the rewrite onto everyone else |
| `git revert` | corrections are hand-written, not machine-generated |
| `git stash`, `git checkout -- <path>`, `git restore`, `git clean` on work you did not make | destroys someone else's uncommitted work |

1. A mistake in a commit — wrong content, wrong message, a claim that turned out false — is
   fixed by a **new commit** that says what was wrong and corrects it.
2. This holds **even when the commit looks unpushed.** Push state changes without you seeing it.
3. **Chain edit-then-commit with `&&`** so a failed edit can never be followed by a commit that
   claims it worked: `edit_files.sh && git commit -m "…"`.
4. **A commit message is a factual claim about the tree.** If it says a file was updated,
   re-read that file before writing the message.

> **Why this rule exists** (the incident behind it, in the project this skill came from,
> 2026-07-18): a scripted edit hit an assertion and wrote nothing, but the unchained `git commit`
> on the next line ran anyway — a commit whose message claimed updates it did not contain. It had
> already been pushed; "fixing" it with `--amend` diverged `main` from `origin/main` and forced a
> force-push. A follow-up commit would have cost nothing.

> Full rule: [`../../../CLAUDE.md`](../../../CLAUDE.md) § Git discipline.

### Rule C — Plan lifecycle: `completed/` and `abandoned/` <!-- hp:lifecycle -->

1. When a plan and its tracker reach 100% (every row `done`, `skipped` or `invalid` with a
   reason; checklist ticked against evidence), move them **immediately**, in the same commit as
   the SERIES update, and fix links that pointed at the old path:
   ```bash
   mkdir -p context/plans/completed
   git mv context/plans/NN_name.md context/plans/completed/NN_name.md
   git mv context/plans/NN_name_tracker.md context/plans/completed/NN_name_tracker.md
   ```
2. **A partially complete plan does not move.**
3. A plan dropped before completion moves to `abandoned/` **with its reason recorded in
   SERIES.md**. A plan dropped without a stated reason gets re-attempted six months later by
   someone who doesn't know it failed.
4. Before starting a plan, every other `in-progress` plan must be listed in SERIES under
   *Currently Active*, or marked `blocked`/`deferred` there with a reason.
5. **A pending plan is a hypothesis, not a contract.** Before starting one, re-read it against
   what has landed since it was written; if anything it relies on changed, update it and add a
   `Revised` line.

### Rule D — Harvest the knowledge before you close the plan <!-- hp:harvest -->

A plan is not done when the code works. It is done when what you learned is on disk.

1. Record findings **as you go**, in the same commit as the task that found them. Where each
   kind of finding goes is defined in `context/AGENTS.md`.
2. Anything that took more than one attempt to understand or fix (not a formatter re-run)
   becomes a pitfall entry.
3. At close, check that every finding the tracker mentions actually landed.
4. **Never claim a finding is recorded unless the bytes are in the file.**

### Rule E — Evidence over assertion <!-- hp:evidence -->

1. **Reality is the oracle** — the running system, the upstream source, the measured output. A
   test that asserts our own values proves nothing about correctness.
2. **A plan premise is a claim.** When reality contradicts the plan: record the measurement, do
   what reality does, and mark the plan item `invalid` as written — in the tracker and in the
   commit body. Do not bend the code to the plan.
3. **Below the bar is a row, not a silence.** Anything left unfinished, unverified or
   disagreeing gets a Follow-ups row in the tracker, naming this plan or a later one.
4. "Inconclusive" is a valid, recordable outcome. Never round it up to a win.
5. Cite the source of a fact as `path:line` or the exact command that showed it.

---

## Project Rules <!-- hp:project-rules -->

Project-specific rules, numbered **P1, P2, …**. They may tighten the core rules above, never
loosen them. This section belongs to the project; `hands-plan resync` never rewrites it.

_None yet — add **P1** when a project-specific rule earns its place._

---

## Series-Scoped Rules <!-- hp:series-rules -->

When several plans share a decision rule (how to reuse code across components, a format both
sides must agree on, an extraction strategy), write it **once** as `context/plans/<TOPIC>_RULES.md`
naming the plan range it governs, link it from each of those plans' headers and from SERIES, and
retire it when the series closes.

Red flag in any plan: **"copy from X" / "port from X" without saying whether the code is
extracted to a shared home, kept where it is, or used only as a reference.** Fix the plan first.

---

## Content Style

- **Bold** for important terms; `code` for file names, variable names, commands.
- Dates always ISO format: `YYYY-MM-DD`.
- Absolute paths preferred in doc sections; relative paths acceptable inside task code blocks.
- Code blocks always carry a language specifier (` ```rust `, ` ```bash `, etc.).
- Cross-reference other plans as "Plan N" or "Plan N T2".

---

## Canonical Template

Use `context/plans/NN_example.md` as the template for every new plan. Copy it, rename it to
`NN_name.md` (with the next zero-padded plan number), and fill in all sections.

For historical context and real examples of the live format, browse `context/plans/completed/`.
Plans 60–67 are the most recent and reflect current conventions.

---

## Mandatory Header in Every New Plan

Every plan file must include this reminder block immediately after the metadata block:

```markdown
> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.
```
