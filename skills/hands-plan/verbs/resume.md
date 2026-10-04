# verb: resume — work a plan to the end, under the rules

`resume [--dir P] [NN | NN-MM | all] [--one] [--ralph]`

Picks up a plan where the tracker says it stands and works **every remaining task**, one at a time
(implement → project gate → tracker → commit, then the next), until all rows are closed or it is
seriously blocked (the stop conditions below). It stops at the end of the plan; a range or `all`
closes each finished plan and moves on to the next (see Multi-plan runs). `--one` does only the next
task and stops. `--ralph` skips the work and prints a self-contained `/ralph-loop` command that
executes the rest of the plan task by task.

## 1. Pick the plan(s)

`NN` given → that plan. Otherwise: the single `in-progress` plan (hp-scan `PLAN … active`), else
the first entry under SERIES *Currently Active*, else the lowest-numbered pending plan whose
dependencies are done — and if that is still ambiguous, ask. A plan in `completed/` or `abandoned/`
is not resumable; say so.

`NN-MM` (or the user asking in plain words for "plans N through M") → those plans in number order.
`all` (or "do all plans") → every pending/in-progress plan in SERIES order, dependencies first. A
plan in the range that is already completed/abandoned → skip it and say so. Only these forms run
more than one plan; anything else stops at the end of one.

## 2. Load the rules and the state — in full

Read: the project's root agent file (and the parent's, in a sub-project), `context/plans/RULES.md`
(**all of it**), any `*_RULES.md` named in the plan header, the plan, its tracker,
`context/AGENTS.md`, and the headings of the pitfalls file (read the entries that touch this
task). The project's RULES governs; if it disagrees with the skill's templates, follow the project.

## 3. Premise refresh (pending plans)

If no task has started yet, the plan is a hypothesis (RULES lifecycle rule): check whether
anything it relies on changed since it was written. Use the commit that added the plan as the base
— **not** a date (`git log --since=<date>` misses same-day commits):

```bash
base=$(git log --diff-filter=A --format=%h -1 -- context/plans/NN_name.md)
git log --oneline "$base"..HEAD -- <Critical Files> <files named in Key Facts>
```

Anything there, or a dependency closed after the plan's `Created`/`Revised` date → re-check its Key
Facts and Before snippets against the tree, propose concrete revisions, and on approval apply them
with a `> **Revised**: <date> — <what changed>` line, committed on its own (the RULES `revise`
bookkeeping form) before starting T1, then go on into the task loop. Nothing changed → say so and
go on.

## 4. Working tree

`git status --short`. Note pre-existing changes that are not yours and **leave them alone** — no
stash, checkout, restore or clean. Stage only explicit paths you changed.

## Loop and stop conditions

Steps 5–8 run once per task. After each task's commit, go straight back to step 5 for the next open
row — don't ask, don't pause for confirmation between tasks; a one-line progress note is enough.
Each task still gets its own commit (RULES: one task per commit).

Stop only when:

- (a) every row is done/skipped/invalid — the plan is finished;
- (b) a task is blocked and every remaining task depends on it or is blocked too;
- (c) the project gate fails and the fix is outside the task's scope;
- (d) a decision is genuinely the user's: a destructive or outward-facing action, a plan revision
  that changes scope, an ambiguity the plan and RULES don't settle;
- (e) changes you did not make collide with the task.

A blocked task with independent tasks after it is **not** a stop: mark it `blocked`, commit, and
continue with the next task that doesn't depend on it. `--one` → stop after the first task.

## Multi-plan runs (`NN-MM`, `all`)

- A plan finishes (stop condition a) → run `$SKILL_DIR/verbs/close.md` for it. The user's range
  request is the approval to commit the close. Close's audit finds work gaps → stop the whole run.
- Close's other questions (Follow-ups that name later plans, harvest entries awaiting approval)
  don't pause the run: take the conservative default — write no unapproved entry, edit no other
  plan — and list each one in the final report for the user to decide.
- Then pick the next plan in the range and start from step 2. Its step-3 premise refresh needing a
  `revise` is stop condition (d): report and halt.
- Any stop condition in any plan ends the whole run.

## 5. The next task

The first Progress row that is not closed, in the order the Resume Instructions give (else table
order). Mark it `in-progress` in the tracker. Announce: `Plan NN · TN — <title>`.

**First task of the plan?** Also flip the started state, in this task's commit (RULES
tracker-format): plan metadata `Status: in-progress`, the SERIES row `in-progress` and listed under
*Currently Active* (format in the SERIES template comment: `- **Plan NN** — <title>: in progress,
X/Y tasks done; next TN (…).`), tracker `Start date:` today. Later tasks keep that Currently
Active line current.

## 6. Do it

- Implement exactly the task. Scope creep → stop and record it as a Follow-up or a blocker.
- Run the task's **Verify** and the **project gate**. Show the real output. Fix warnings.
- The task added a behavior the gate should exercise (a flag, a mode, an endpoint)? Update the
  project gate in RULES in this commit (RULES gate rule, "keep the gate current").
- **Premise wrong?** (RULES evidence rule) Record what reality showed (command + output), do what
  reality requires, mark the plan item `invalid` as written in the tracker Notes **and** the commit
  body, and add a Follow-ups row for anything left below the bar.
- Blocked → set the row `blocked` with the reason, record what was learned, commit that, then
  apply the stop conditions (continue if an independent task remains).

## 7. Record

- Tracker: the row → `done` (or `skipped`/`invalid`/`blocked` + reason in its Notes cell),
  Overview `N% (X/Y)`, Notes/Deviations, resolved Open Unknowns (strike + "RESOLVED (TN): …"),
  Evidence rows for anything measured.
- Harvest now (RULES harvest rule): new confirmed facts, things that took more than one attempt to
  understand or fix, dependency choices → the files `context/AGENTS.md` routes them to.
- Plan: tick this task's checklist box only against evidence produced in step 6.

## 8. Commit

Re-read every file the message will describe, then one chained commit with explicit paths — the
work, the tracker, the plan (checklist ticks, started state), SERIES (if it changed), RULES (if the
gate changed) and any context entries together:

```bash
git add <changed paths> context/plans/NN_name_tracker.md context/plans/NN_name.md \
        [context/plans/SERIES.md] [context/plans/RULES.md] [context/<knowledge files>] \
  && git commit -m "<the task's **Commit** line, in the project's format>"
```

Then a one-line report (task, gate result, commit hash) and back to step 5 — unless a stop
condition holds or `--one` was given.

When the run stops, report: tasks done with their hashes, any blocked rows and why, plans closed,
deferred close questions, and what's next. A single plan with all rows closed → suggest
`/hands-plan close NN`.

## 9. `--ralph`: print a loop command (no task work)

Run steps 1–2 (read-only) to pick the plan and count the remaining tasks. If the plan has not
started, run step 3's premise refresh first (it may need a `revise` commit, with approval) — the
loop does not refresh premises. Then print this, filled in; drop the parenthetical about the first
task if T1 has already started. It must not depend on `/hands-plan` (the loop re-feeds the prompt
as plain text, and this skill is user-invoked only):

```text
/ralph-loop "Work Plan NN in <project-dir>. Each iteration: read context/plans/RULES.md in full, then context/plans/NN_name.md and context/plans/NN_name_tracker.md. Take the first task that is not done, in Resume Instructions order. Do only that task (on the first task, also set the plan and its SERIES row to in-progress and the tracker Start date). Pass the task's Verify and the RULES project gate, showing real output; if the task added behavior the gate should cover, update the gate. Update the tracker row (and Notes/Deviations, Evidence, context/ findings) and commit everything in ONE commit using the task's Commit line, staging explicit paths and chaining with &&. Never push, never amend/rebase/reset/revert, never touch changes you did not make. If a plan premise is wrong, record it, follow reality, and mark the item invalid. If a task cannot be done, mark it blocked with the reason and commit. When every task row is done/skipped/invalid, or every remaining one is blocked with a recorded reason, output <promise>PLAN NN HALTED</promise>." --max-iterations <2 × remaining tasks + 2> --completion-promise "PLAN NN HALTED"
```

Remind the user: the loop does not run `close` — run `/hands-plan close NN` after it halts. If
`.gitignore` lacks it, suggest adding `.claude/ralph-loop.local.md` (the plugin's state file;
otherwise it shows up as an untracked file).
