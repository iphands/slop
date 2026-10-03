# verb: status — dashboard + hygiene lint (read-only)

`status [--dir P] [NN]`

**Never writes anything** — no edits, no `mkdir`, no commits. Every finding is paired with the verb
or edit that fixes it; the user decides.

## 1. Gather

```bash
"$SKILL_DIR/bin/hp-scan" "<project-dir>"
date +%F
```

No `context/plans/RULES.md` → say so and suggest `/hands-plan init` (or `resync` if numbered plans
exist without RULES). Read `context/plans/SERIES.md` (header, north star, currently active, the
plans table) and, for each **active** plan from hp-scan, its tracker's Overview, Progress table and
Notes/Deviations (not the whole plan).

## 2. Dashboard

Keep it scannable:

```
<project> — hands-plan <local stamp | legacy> (skill v<N>)
North star: <one line>
Next free plan number: <NN>   (hp-scan computed <NN>)

Active
  NN  <title>   X/Y tasks  next: TN <title>   last activity <date>   [blocked: TN — reason]
Pending (next up): NN <title>, NN <title>, …
Recently closed: NN <title> (<date>), …
```

"next" = the first non-closed Progress row in Resume Instructions order. "last activity" = hp-scan
`last=`.

## 3. Lint

Report each hp-scan `LINT`, `DUP`, `DEADREF`, `SIZE`, `PLACEHOLDER`, `LEFTOVER` record, grouped and
de-duplicated, as *finding → fix*. Heuristic findings say "possibly". Add these checks of your own
from what you read:

| Finding | Fix |
|---|---|
| `done-not-moved` — all rows closed / metadata done, plan still active | `/hands-plan close NN` |
| `no-tracker` / `orphan-tracker` | create the tracker from `NN_example_tracker.md` / find its plan |
| `series-missing`, `series-status`, `series-missing-closed`, `next-free-stale` | edit SERIES (list the exact rows) |
| `stale` — in progress, no tracker/plan commit for > 14 days | resume it, or mark it blocked/deferred with a reason |
| `DUP` plan numbers | report only; never renumber existing files |
| Plan metadata `Status` disagrees with its location (e.g. `pending` in `completed/`) | edit the metadata line |
| Ticked checklist box whose task row isn't closed | untick, or produce the evidence |
| Abandoned plan with no reason in SERIES | add the reason row |
| `blocked`/`skipped`/`invalid` row with no reason in Notes | add the reason |
| `SIZE` SERIES > 30 KB / context file > 40 KB | compact SERIES into `completed/SERIES_ARCHIVE.md`; split the file into a directory + index |
| `DEADREF` in a root/context agent file | fix the path or create the file |
| Root block missing or older than skill v<N>; local stamp older than skill | `/hands-plan resync` |
| Legacy (no stamp) | `/hands-plan resync` shows the migration |

## 4. With `NN`

Detail view for that plan: metadata, every Progress row with status and notes, open questions /
unknowns still unresolved, Notes/Deviations, Follow-ups, the checklist with ticked/unticked, and
the commits for it (`git log --oneline --grep='P<n>]'` or the project's format; fall back to
`git log --oneline -- <plan> <tracker>`).

## 5. End

One line: the single most useful next action (`/hands-plan resume NN`, `close NN`, `new …`, or
`resync`).
