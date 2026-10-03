# verb: resume — do the next task of a plan, under the rules

`resume [--dir P] [NN] [--ralph]`

Picks up a plan where the tracker says it stands, does **one task** end to end (implement → Rule A
gate → tracker → commit), and stops. `--ralph` skips the work and prints a self-contained
`/ralph-loop` command that executes the rest of the plan task by task.

## 1. Pick the plan

`NN` given → that plan. Otherwise: the single `in-progress` plan (hp-scan `PLAN … active`), else
the first entry under SERIES *Currently Active*, else the lowest-numbered pending plan whose
dependencies are done — and if that is still ambiguous, ask. A plan in `completed/` or `abandoned/`
is not resumable; say so.

## 2. Load the rules and the state — in full

Read: the root agent file, `context/plans/RULES.md` (**all of it**), any `*_RULES.md` named in the
plan header, the plan, its tracker, `context/AGENTS.md`, and the headings of the pitfalls file
(read the entries that touch this task). The project's RULES governs; if it disagrees with the
skill's templates, follow the project.

## 3. Premise refresh (pending plans)

If no task has started yet and the plan predates work it depends on (a dependency closed after the
plan's `Created`/`Revised` date, or files in Critical Files changed since — check
`git log --since=<date> -- <files>`), the plan is a hypothesis: re-check its Key Facts and Before
snippets against the tree. Propose concrete revisions; on approval apply them, add a
`> **Revised**: <date> — <what changed>` line, and commit that on its own (plan-doc commit in the
project's format) before starting T1.

## 4. Working tree

`git status --short`. Note pre-existing changes that are not yours and **leave them alone** — no
stash, checkout, restore or clean. Stage only explicit paths you changed.

## 5. The next task

The first Progress row that is not closed, in the order the Resume Instructions give. Mark it
`in-progress` in the tracker. Announce: `Plan NN · TN — <title>`.

## 6. Do it

- Implement exactly the task. Scope creep → stop and record it as a Follow-up or a blocker.
- Run the **Rule A project gate** and the task's **Verify**. Show the real output. Fix warnings.
- **Premise wrong?** (RULES `hp:evidence`) Record what reality showed (command + output), do what
  reality requires, mark the plan item `invalid` as written in the tracker Notes **and** the commit
  body, and add a Follow-ups row for anything left below the bar.
- Blocked → set the row `blocked` with the reason, record what was learned, commit that, stop.

## 7. Record

- Tracker: the row → `done` (or `skipped`/`invalid`/`blocked` + reason), Overview percentage
  `N% (X/Y)`, Notes/Deviations, resolved Open Unknowns (strike + "RESOLVED (TN): …"), Evidence rows
  for anything measured.
- Harvest now (RULES `hp:harvest`): new confirmed facts, multi-attempt pitfalls, dependency choices
  → the files `context/AGENTS.md` routes them to.
- Plan checklist: tick this task's box only against evidence produced in step 6.

## 8. Commit

Re-read every file the message will describe, then one chained commit with explicit paths — the
work, the tracker row and any context entries together:

```bash
git add <changed paths> context/plans/NN_name_tracker.md [context/<knowledge files>] \
  && git commit -m "<the task's **Commit** line, in the project's format>"
```

Then report: what changed, the gate output summary, the commit hash, what's next. Continue to the
next task **only if the user asked for more than one**. All rows closed → suggest
`/hands-plan close NN`.

## 9. `--ralph`: print a loop command (do no work)

Print this, filled in — it must not depend on `/hands-plan` (the loop re-feeds the prompt as plain
text, and this skill is user-invoked only):

```text
/ralph-loop "Work Plan NN in <project-dir>. Each iteration: read context/plans/RULES.md in full, then context/plans/NN_name.md and context/plans/NN_name_tracker.md. Take the first task that is not done, in Resume Instructions order. Do only that task. Pass the Rule A project gate and the task's Verify, showing real output. Update the tracker row (and Notes/Deviations, Evidence, context/ findings) and commit everything in ONE commit using the task's Commit line, staging explicit paths and chaining with &&. Never push, never amend/rebase/reset/revert, never touch changes you did not make. If a plan premise is wrong, record it, follow reality, and mark the item invalid. If a task cannot be done, mark it blocked with the reason and commit. When every task row is done/skipped/invalid, or every remaining one is blocked with a recorded reason, output <promise>PLAN NN HALTED</promise>." --max-iterations <2 × remaining tasks + 2> --completion-promise "PLAN NN HALTED"
```

Remind the user: the loop does not run `close` — run `/hands-plan close NN` after it halts.
