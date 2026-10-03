# verb: new — draft the next plan + tracker

`new [--dir P] [--parent NN] [--series-rules TOPIC] <what the work is>`

Researches the work, writes `context/plans/NN_name.md` + `NN_name_tracker.md` in the project's own
format, registers the plan in SERIES, and (after asking) commits the three files. A plan is only as
good as its grounding: real paths, real Before code, Key Facts that were actually checked.

## 1. Preconditions

- `context/plans/RULES.md` must exist; otherwise say so and suggest `/hands-plan init`.
- No description → ask what the work is (one or two sentences). Don't invent scope.
- Run `"$SKILL_DIR/bin/hp-scan" "<project-dir>"` and `date +%F`.

## 2. Read the project's rules — in full

Read, in this order: `context/plans/RULES.md` (**all of it** — the project's version governs, not
the skill's templates), `context/plans/SERIES.md`, `context/plans/NN_example.md`,
`context/plans/NN_example_tracker.md` (if missing — a legacy project — use the tracker format
described in RULES), `context/AGENTS.md` if present, and any `context/plans/*_RULES.md` whose plan
range covers this work. Skim the headings of `context/pitfalls.md` / `distilled.md` (or their
equivalents) and read the entries relevant to this work.

## 3. Does this need a plan?

Apply RULES' *When Does a Change Need a Plan?* table (or its legacy equivalent). If it doesn't,
say so, describe the direct path (do → Rule A gate → commit), and stop — unless the user insists.

## 4. Lifecycle check

From hp-scan `PLAN … active` and SERIES *Currently Active*: every other `in-progress` plan must be
listed as active in SERIES, or marked `blocked`/`deferred` with a reason. If one isn't, ask:
**continue in parallel** (list both as active) / **mark it blocked or deferred** (with what reason) /
**stop and finish it first**. Report hp-scan `LINT done-not-moved` plans — suggest
`/hands-plan close NN`.

## 5. Find the slot

- Mine earlier work first: Follow-ups rows in recent trackers (active and `completed/`), SERIES
  backlog/pending rows. If this work **is** an existing pending row, reuse its number and title
  rather than creating a duplicate.
- `--parent NN` (or the work is clearly a piece of plan NN) → sub-plan `NN_<k>_name.md`, `k` = next
  free sub-number for NN.
- Otherwise the number is hp-scan `NEXT computed=` (it already accounts for `completed/`,
  `abandoned/` and numbers reserved by SERIES rows). If SERIES' *Next free plan number* disagrees,
  report it and use the larger.
- Name: short snake_case of the deliverable (`07_retry_backoff`), not of the activity.

## 6. Research — ground every section

Use Explore subagents in parallel when the area is wide; read files directly when it is narrow.

- **Critical Files**: real paths that exist (or are marked *new*). **Before** snippets copied from
  the current tree, with enough context to locate the hunk.
- **Key Facts**: run the cheap probes now (a command, a grep, an API call) and record *how
  confirmed* with the date. Anything not checked is written `unconfirmed — confirm in T1`, and T1
  confirms it.
- **Pre-Identified Bug**: reproduce it and paste the command + verbatim output. Can't reproduce →
  say so; the first task becomes the reproduction.
- **Why [Approach]**: name the alternative you rejected and why.
- **Rejected Claims**: anything that looked like a bug and isn't — with the evidence.
- **Verify** for each task: derived from RULES' Rule A project gate plus the behavior this task
  changes. A Verify that only repeats the build is not enough.
- **Expected observation** on every measurement/investigation task, written now.
- Shared decision across several plans (e.g. how code gets reused between components) → offer a
  series rules file (step 8b).

Ask the user (AskUserQuestion) about real design forks you can't settle from the code. Don't ask
what the code already answers.

## 7. Write the plan and tracker

- Copy the project's `NN_example.md` → `context/plans/NN_name.md`. **Drop the first-line
  `hands-plan` stamp**, delete the instructional `<!-- … -->` comments, fill every section; a
  section that doesn't apply says `N/A — <reason>`. Metadata: `Status: pending`, `Created:` today,
  `Depends on:` real plan numbers, one-sentence `Goal`.
- Every task: `### TN: …` with File, What to do, Before/After (or the target symbol + evidence
  source when the edit isn't known yet), **Verify**, Expected observation (where applicable), and a
  **Commit** line in the project's format. Large plans: waves with exit gates.
- Copy `NN_example_tracker.md` → `NN_name_tracker.md` (stamp dropped): Progress rows **1:1** with
  the tasks, numbered Resume Instructions specific to this plan (what to read, environment,
  ordering, commit format with the row in the same commit), Evidence table kept only if the plan
  produces numbers or live checks.

## 8. Register in SERIES

- Add the plan's row (Notes ≤ ~25 words), update the dependency chain / milestones / currently
  active sections **if the file has them**, bump **Next free plan number** (add the line if SERIES
  lacks it — it's part of the structure).
- Don't rewrite other rows.

### 8b. Series rules (only with `--series-rules TOPIC` or on approval)

Copy `$SKILL_DIR/templates/plans/TOPIC_RULES.md` → `context/plans/<TOPIC>_RULES.md`, fill the plan
range and the decision map from the research, link it from the plan header and the SERIES row.

## 9. Self-check before showing anything

```bash
"$SKILL_DIR/bin/hp-scan" "<project-dir>"
```

- No `LEFTOVER` / `PLACEHOLDER` records for the new files (`[Title]`, `Plan NN`, `YYYY-MM-DD`,
  `{{…}}`, a stamp).
- Every task has **Verify** and **Commit**; tracker rows == tasks; every `Depends on` plan exists;
  every Critical File exists or is marked new.
- `NEXT computed=` is now one past the new plan, and SERIES says the same.

## 10. Show, ask, commit

Show the TL;DR, the task list (one line each) and anything still unconfirmed. Ask: **Commit the
plan (Recommended)** / **Let me review first**. On yes — re-read the three files, then:

```bash
git add context/plans/NN_name.md context/plans/NN_name_tracker.md context/plans/SERIES.md [<TOPIC>_RULES.md] \
  && git commit -m "<plan-doc commit in the project's format, e.g. [P<n>][plan] add <title>>"
```

## 11. Next

`/hands-plan resume NN` — or hand the printed ralph command (resume step 9) to a loop.
