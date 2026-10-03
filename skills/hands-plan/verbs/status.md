# verb: status — dashboard + hygiene lint (read-only)

`status [--dir P] [NN]`

**Never writes anything** — no edits, no `mkdir`, no commits. Every finding is paired with the verb
or edit that fixes it; the user decides.

## 1. Gather

```bash
"$SKILL_DIR/bin/hp-scan" --brief .        # drop --brief for a small project, or for `status NN`
git log -1 --format='%h %cr' ; git status --short | wc -l
date +%F
```

No `context/plans/RULES.md` → say so and suggest `/hands-plan init` (its adopt mode handles
numbered plans that have no RULES). Read `context/plans/SERIES.md` (header, north star, currently
active, the plans table) and, for each **active** plan from hp-scan, its tracker's Overview,
Progress table and Notes/Deviations (not the whole plan).

Definitions: **active** = plan files in `context/plans/` (any status); **in progress** = status
`in-progress` or any closed tracker row; **pending** = active with no row started.

## 2. Dashboard

```
<project> — hands-plan <RULES.md stamp | legacy> (skill v<N>)    HEAD <hash> (<age>), <k> uncommitted files
North star: <one line; SERIES "North star", else its opening arc statement, else "— (none recorded)">
Next free plan number: <SERIES value | — (no line)>   (hp-scan computed <NN>)

In progress
  NN  <title>   X/Y tasks   next: TN <title>   last activity <date>   [blocked: TN — reason]
Pending
  NN  <title>   0/Y tasks   [plan-level block: <Overview "Blocked on" line, if any>]
Recently closed (last 5): NN <title> (<closed date>, completed|abandoned), …
```

- "next" = the first non-closed Progress row in Resume Instructions order, else table order; all
  rows closed → `— (all closed → /hands-plan close NN)`.
- "last activity" = hp-scan `last=`; "closed date" = hp-scan `closed=` (`git log` add/rename date).
- Uncommitted files you didn't create, or HEAD moving while you read → add one line: *another
  session may be working here; this view can go stale.*

## 3. Lint

Report each hp-scan `LINT`, `DUP`, `DEADREF`, `SIZE`, `PLACEHOLDER`, `LEFTOVER` record, grouped and
de-duplicated, as *finding → fix*. `LINT` kinds `stale`, `series-*`, `meta-*` and `done-not-moved`
are heuristics — say "possibly". Plus these checks from what you read:

| Finding | Fix |
|---|---|
| `done-not-moved` — all rows closed / metadata done, plan still active | `/hands-plan close NN` |
| `meta-behind` — metadata `pending` but rows closed | set plan Status and the SERIES row to `in-progress` |
| `no-tracker` / `orphan-tracker` | create the tracker from `NN_example_tracker.md` / find its plan |
| `series-missing`, `series-status`, `series-dup-row`, `next-free-stale` | edit SERIES (list the exact rows) |
| `series-missing-closed`, `meta-location-closed` | **archive drift — one INFO line**, no per-plan fixes (closed plans are history) |
| `stale` — in progress, no tracker/plan commit for > 14 days; or commits carrying the plan's tag landed after the tracker's last touch (`git log -F --grep='[P<n>]' -- .`) | resume it, or mark it blocked/deferred with a reason |
| `DUP` plan numbers | report only; never renumber existing files |
| Ticked checklist box whose task row isn't closed (active plans) | untick, or produce the evidence |
| Abandoned plan with no reason in SERIES | add the reason row |
| `blocked`/`skipped`/`invalid` row with no reason in its Notes cell (active plans) | add the reason |
| `SIZE` SERIES > 30 KB / context or root file > 40 KB | compact SERIES into `completed/SERIES_ARCHIVE.md`; split the file into a directory + `index.md` |
| `DEADREF` | fix the path, or create the file it names |
| Root block missing or older than skill v<N>; RULES stamp older than skill v<N> | `/hands-plan resync` |
| Legacy (no stamp) | `/hands-plan resync` shows the migration |

## 4. With `NN`

Detail view for that plan: metadata, every Progress row with status and notes, open questions /
unknowns still unresolved, Notes/Deviations, Follow-ups, the checklist with ticked/unticked, and
its commits (`git log --oneline -F --grep='[P<n>]' -- .` in the project's format; fall back to
`git log --oneline -- <plan> <tracker>`).

## 5. End

One line: the single most useful next action (`/hands-plan resume NN`, `close NN`, `new …`, or
`resync`).
