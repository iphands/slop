# verb: close — complete or abandon a plan

`close [--dir P] NN [--abandon "reason"]`

Moves a finished plan (and its tracker) to `completed/`, or a dropped one to `abandoned/`, after an
audit and a knowledge harvest, and updates SERIES — in **one** commit. A partially complete plan
does not move.

## 1. Load

`NN` is required (ask if missing). Read `context/plans/RULES.md` (lifecycle, harvest and evidence
sections at least), the plan, its tracker, SERIES, and `context/AGENTS.md` (routing). Run
`"$SKILL_DIR/bin/hp-scan" "<project-dir>"` and `date +%F`. The plan must be active (in
`context/plans/`); a plan already in `completed/`/`abandoned/` → say so and stop.

## 2a. `--abandon "reason"`

A reason is mandatory. Then:

1. Every open tracker row → `skipped` with the reason (or `blocked` if it was blocked).
2. Harvest anyway (step 4) — negative results and dead ends are knowledge.
3. SERIES: the plan's row → `abandoned`; add a row to *Abandoned / Superseded* (reason, superseded
   by, date); remove it from *Currently Active*.
4. Plan metadata `Status: abandoned`; tracker Overview notes the reason.
5. Move and commit (step 6), using `abandoned/` instead of `completed/`.

## 2b. Completion audit — all must hold

| Check | How |
|---|---|
| Every tracker row closed | `done`, or `skipped`/`invalid` **with a reason**. `blocked`/`pending` rows → not closable. |
| Every done task was committed | `git log --oneline` grep for the plan's tag (e.g. `[P<n>]`), or the tracker's history (`git log -- <tracker>`). A done row with no commit → gap. |
| Checklist ticked against evidence | Each box has evidence in the tracker or the commit history. Unticked → produce the evidence now (run it) or report the gap. |
| Open questions | Each resolved in place or marked `deferred — <reason>`. |
| Below-the-bar work | Every unfinished/unverified item has a Follow-ups row naming where it lands. |
| Rule A on HEAD | Run the project gate once more on the current tree; it must pass. |

Any failure → **report the gaps and stop.** Do not move anything; suggest `/hands-plan resume NN`.

## 3. Follow-ups → SERIES

Each Follow-ups row that names a later plan: make sure SERIES has that plan (add a `pending` row
with ≤ 25 words of notes if it doesn't — don't bump numbers already reserved). Add an
`After **NN**: <capability>` line if SERIES keeps Milestones.

## 4. Harvest (RULES `hp:harvest`)

Collect what the plan learned — Notes/Deviations, Evidence, invalid premises, multi-attempt fixes,
confirmed facts, dependency decisions. Check each against the knowledge files `context/AGENTS.md`
routes it to. For each one **not yet on disk**, propose the entry (file + text, in that file's
entry shape). Ask once; write the approved entries; **re-read each file to confirm the bytes are
there.** Sub-project with a parent `context/` → cross-cutting entries also go up.

## 5. Mark done

Plan metadata `Status: done`; tracker Overview `Status: 100% complete (Y/Y)`; tick the checklist
closers (harvest done, moved + SERIES updated) — they become true in this commit. SERIES: row →
`done` with a one-line outcome, removed from *Currently Active*, next active plan named if there is
one.

## 6. Move + commit — one chained command

Re-read SERIES, the plan, the tracker and every knowledge file you touched, then:

```bash
mkdir -p context/plans/completed \
  && git mv context/plans/NN_name.md context/plans/completed/NN_name.md \
  && git mv context/plans/NN_name_tracker.md context/plans/completed/NN_name_tracker.md \
  && git add context/plans/completed/NN_name.md context/plans/completed/NN_name_tracker.md \
             context/plans/SERIES.md <harvested knowledge files> \
  && git commit -m "<project format, e.g. [P<n>][close] <title>: <one-line outcome>>"
```

(`git mv` stages the rename; the `git add` stages the edits made to the moved files.) Ask before
committing if the user hasn't already asked for the close to be committed.

## 7. Report

Commit hash, what was harvested (file → entry title), follow-ups registered, and the next plan:
`/hands-plan resume NN` or `/hands-plan new …`.
