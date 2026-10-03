# verb: new — draft the next plan + tracker

`new [--dir P] [--parent NN] [--series-rules TOPIC] <what the work is>`

Researches the work, writes `context/plans/NN_name.md` + `NN_name_tracker.md` in the project's own
format, registers the plan in SERIES, and (after asking) commits the three files. A plan is only as
good as its grounding: real paths, real Before code, Key Facts that were actually checked.

## 1. Preconditions

- `context/plans/RULES.md` must exist; otherwise say so and suggest `/hands-plan init` (its adopt
  mode handles numbered plans without RULES).
- No description → ask what the work is (one or two sentences). Don't invent scope.
- Run `"$SKILL_DIR/bin/hp-scan" --brief .` and `date +%F`.
- **Migrate mode:** the description is a plan-system migration ("migrate the plan system to
  hands-plan vN", following a `resync`). Then: use the resync checklist from this conversation (or
  run `resync` first if there is none); one task per checklist group, CONFLICTs first, each task's
  Verify = the relevant `hp-probe` / `hp-scan` checks passing; the last task adds the
  `<!-- hp:<id> -->` marker to **every** mapped heading (untouched sections too — otherwise the
  next resync can't find them) and stamps `hands-plan:v<N>` on RULES, SERIES, both `NN_example*`
  files, `context/AGENTS.md` and any `*_RULES.md` it touched, and the root block's begin marker. Skip step 3 (the gate may not exist
  yet); if the project lacks `NN_example*.md`, use `$SKILL_DIR/templates/plans/` as the skeleton.
  Check active plans for edits to RULES/SERIES (their Critical Files) and sequence around them.

## 2. Read the project's rules — in full

`context/plans/RULES.md` (**all of it** — the project's version governs, not the skill's
templates), `context/plans/SERIES.md`, `context/plans/NN_example.md` and
`context/plans/NN_example_tracker.md` (a legacy project missing either → use the shape RULES
describes, or `$SKILL_DIR/templates/plans/`), `context/AGENTS.md` if present, and any
`context/plans/*_RULES.md` whose plan range covers this work. Skim the headings of the
pitfalls/distilled files (hp-scan `KNOWLEDGE`) and read the entries relevant to this work.

## 3. Does this need a plan?

Apply RULES' *When Does a Change Need a Plan?* table (or its legacy equivalent). If it doesn't,
say so, describe the direct path (do → project gate → commit), and stop — unless the user insists.

## 4. Lifecycle check

From hp-scan `PLAN … active` and SERIES *Currently Active*: every other `in-progress` plan must be
listed as active in SERIES, or marked `blocked`/`deferred` with a reason. If one isn't, ask:
**continue in parallel** (list both as active) / **mark it blocked or deferred** (with what reason) /
**stop and finish it first** (a project rule may require this — follow it). Report hp-scan
`LINT done-not-moved` plans — suggest `/hands-plan close NN`.

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
**Probes leave no artifacts** (SKILL.md global rules) — a probe that writes caches or build output
into the tree must clean up only what it created, or run elsewhere.

- **Critical Files**: real paths that exist (or are marked *new*). **Before** snippets copied from
  the current tree, with enough context to locate the hunk.
- **Key Facts**: run the cheap probes now (a command, a grep, an API call) and record *how
  confirmed* with the date. Anything not checked is written `unconfirmed — confirm in T1`, and T1
  confirms it.
- **Pre-Identified Bug/Issue**: reproduce it and paste the command + verbatim output. For a
  feature, the command showing today's behavior. Trim machine-specific absolute paths that don't
  matter. Can't reproduce → say so; the first task becomes the reproduction.
- **Why [Approach]**: name the alternative you rejected and why.
- **Rejected Claims**: anything that looked like a bug and isn't — with the evidence.
- **Verify** for each task: the command that exercises *this task's* change and its expected
  output, then the project gate. A Verify that only repeats the build is not enough.
- **Expected observation** on every measurement/investigation task, written now.
- **The gate can't run** (missing tools, failing baseline): ask — **first task makes it runnable**
  vs **an environment prerequisite** the human handles (record it in SERIES Standing Constraints).
- An open question you decide to defer is marked `deferred — <reason>` now, not at close.
- Shared decision across several plans (e.g. how code gets reused between components) → offer a
  series rules file (step 8b).

Ask the user (AskUserQuestion) about real design forks you can't settle from the code. Don't ask
what the code already answers.

## 7. Write the plan and tracker

- Copy the project's `NN_example.md` → `context/plans/NN_name.md`. **Drop the first-line
  `hands-plan` stamp**, delete the instructional `<!-- … -->` comments, fill every required
  section (`N/A — <reason>` if it doesn't apply); optional sections may go. Metadata:
  `Status: pending`, `Created:` today, `Depends on:` real plan numbers, one-sentence `Goal`.
- Every task: `### TN: …` with File, What to do, Before/After (or the target symbol + evidence
  source when the edit isn't known yet), **Verify**, Expected observation (where applicable), and a
  **Commit** line in the project's format with the real plan number (`[P7][T2][parser] …`, not
  `[P07]` unless the project pads). Large plans: waves with exit gates.
- Copy `NN_example_tracker.md` → `NN_name_tracker.md` (stamp dropped): Progress rows **1:1** with
  the tasks, `Start date: — (set when T1 starts)`, numbered Resume Instructions specific to this
  plan (what to read, environment, ordering, commit format with the row in the same commit),
  Evidence table kept only if the plan produces numbers or live checks.

## 8. Register in SERIES

- Add the plan's row (`pending`, Notes ≤ ~25 words), update the dependency chain / milestones
  **if the file has them**, bump **Next free plan number** (add the line if SERIES lacks it).
- *Currently Active*: if nothing is in progress, replace "None yet …" with
  `None in progress — next up: Plan NN (\`/hands-plan resume NN\`).` Don't list a pending plan as
  active.
- Don't rewrite other rows.

### 8b. Series rules (only with `--series-rules TOPIC` or on approval)

Copy `$SKILL_DIR/templates/plans/TOPIC_RULES.md` → `context/plans/<TOPIC>_RULES.md`, fill
`{{DATE}}`, the plan range and the decision map from the research, link it from the plan header
and the SERIES row.

## 9. Self-check before showing anything

```bash
"$SKILL_DIR/bin/hp-scan" --brief .
```

- No `LEFTOVER` / `PLACEHOLDER` records for the new files (`[Title]`, `Plan NN`, `YYYY-MM-DD`,
  `{{…}}`, a stamp).
- Every task has **Verify** and **Commit**; tracker rows == tasks; every `Depends on` plan exists;
  every Critical File exists or is marked new.
- `NEXT computed=` is now one past the new plan, and SERIES says the same.

## 10. Show, ask, commit

Show the TL;DR, the task list (one line each) and anything still unconfirmed. Ask: **Commit the
plan (Recommended)** / **Let me review first**. On yes — re-read the files, then:

```bash
git add context/plans/NN_name.md context/plans/NN_name_tracker.md context/plans/SERIES.md [context/plans/<TOPIC>_RULES.md] \
  && git commit -m "<the RULES bookkeeping form, e.g. [P7][plan] add retry backoff>"
```

(Projects whose commit format predates bookkeeping forms: use their own plan-doc style if
`git log` shows one, e.g. `plan(NN): …`; otherwise the plan tag + `plan`.)

## 11. Next

`/hands-plan resume NN` — or `/hands-plan resume NN --ralph` for a loop command.
