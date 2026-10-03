# verb: close — complete or abandon a plan

`close [--dir P] NN [--abandon "reason"]`

Moves a finished plan (and its tracker) to `completed/`, or a dropped one to `abandoned/`, after an
audit and a knowledge harvest, and updates SERIES — in **one** commit. A partially complete plan
does not move.

## 1. Load

`NN` is required (ask if missing). Read `context/plans/RULES.md` (lifecycle, harvest and evidence
rules at least), the plan, its tracker, SERIES, and `context/AGENTS.md` (routing). Run
`"$SKILL_DIR/bin/hp-scan" --brief .` and `date +%F`. The plan must be active (in
`context/plans/`); one already in `completed/`/`abandoned/` → say so and stop.

## 2a. `--abandon "reason"`

A reason is mandatory. Then:

1. Every open tracker row → `skipped`, with the reason in its Notes cell (or `blocked` if it was
   blocked); the Overview line becomes `Status: abandoned — <reason> (X/Y rows closed)`.
2. Unticked checklist boxes stay unticked, with one note: "abandoned — not verified".
3. Follow-ups: register in SERIES only those still relevant after the abandonment.
4. Harvest anyway (step 4) — negative results and dead ends are knowledge.
5. Plan metadata `Status: abandoned`. SERIES: the row → `abandoned`, a row in *Abandoned /
   Superseded* (reason, superseded by — free text if the successor has no plan yet — date), and
   *Currently Active* updated (remove it, or its "next up" mention).
6. Move and commit (steps 6–7) into `abandoned/` instead of `completed/`.

## 2b. Completion audit

| Check | How |
|---|---|
| Every tracker row closed | `done`, or `skipped`/`invalid` **with a reason**. `blocked`/`pending` rows → not closable. |
| Every done task was committed | Fixed-string, path-scoped grep for the plan's tag — `git log --oneline -F --grep='[P<n>]' -- .` (adapt to the project's format; `-F`, because `[P1]` is a regex character class otherwise) — or the tracker's history, `git log --oneline -- <tracker>`. A done row with no commit → gap. |
| Checklist ticked against evidence | Each box has evidence in the tracker or the commit history. Unticked → produce the evidence now (run it), or report the gap. |
| Open questions | Each resolved in place or marked `deferred — <reason>`. |
| Below-the-bar work | Every unfinished/unverified item has a Follow-ups row naming where it lands. |
| Project gate on HEAD | Run it once more on the current tree; it must pass. |

**Work gaps** (an open row, an uncommitted task, a failing gate, missing evidence that can't be
produced now) → report them and stop; move nothing; suggest `/hands-plan resume NN`.
**Document gaps** whose substance is already recorded (a question answered in prose but not marked
`deferred`/RESOLVED, a missing Follow-ups row for a known gap) → normalize them in the close commit
and list what you normalized.

## 3. Follow-ups → SERIES

Each Follow-ups row that names a later plan — by number, or as "a later plan": ask whether to
register it as a SERIES `pending` row (≤ 25 words; this reserves a number and moves Next free) or
leave it in the tracker only. Add an `After **NN**: <capability>` line if SERIES keeps Milestones.

## 4. Harvest (RULES harvest rule)

Collect what the plan learned — Notes/Deviations, Evidence, invalid premises, things that took more
than one attempt, confirmed facts, dependency decisions. Check each against the knowledge files
`context/AGENTS.md` routes it to. For each one **not yet on disk**, propose the entry (file + text,
in that file's entry shape). Ask once; write the approved entries; **re-read each file to confirm
the bytes are there.** Sub-project with a parent `context/`: cross-cutting entries are *proposed*
for the parent too, and written only on their own approval (outside the project).

## 5. Mark done

Plan metadata `Status: done`; tracker Overview `Status: 100% complete (Y/Y)`; tick the checklist
closers (harvest done, moved + SERIES updated) — they become true in this commit. SERIES: row →
`done` with a one-line outcome, removed from *Currently Active*, the next active plan named if
there is one.

## 6. Move — and fix the links

```bash
mkdir -p context/plans/completed \
  && git mv context/plans/NN_name.md context/plans/completed/NN_name.md \
  && git mv context/plans/NN_name_tracker.md context/plans/completed/NN_name_tracker.md
```

Then update everything that pointed at the old paths: the SERIES row's link, the tracker's
`Plan:` line, the plan's references to its tracker.

## 7. Commit — one chained command

Re-read SERIES, the plan, the tracker and every knowledge file you touched; ask before committing
if the user hasn't already asked for the close to be committed; then:

```bash
git add context/plans/completed/NN_name.md context/plans/completed/NN_name_tracker.md \
        context/plans/SERIES.md <harvested knowledge files> [<approved parent files>] \
  && git commit -m "<the RULES bookkeeping form, e.g. [P7][close] retry backoff: <one-line outcome>>"
```

(`git mv` stages the rename; the `git add` stages the edits to the moved files.)

## 8. Report

Commit hash, what was harvested (file → entry title), follow-ups registered, documents normalized,
and the next plan: `/hands-plan resume NN` or `/hands-plan new …`.
