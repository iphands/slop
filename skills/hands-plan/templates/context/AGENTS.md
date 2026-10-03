<!-- hands-plan:v1 {{DATE}} -->
# context/ — {{PROJECT_NAME}}'s living memory

You are reading inside `context/`, the project's knowledge base. Agents that skip it repeat
solved problems; agents that don't write to it leave traps for the next one. **Read the relevant
files before new work. Write what you learn as you go — in the same commit as the work.**

`plans/RULES.md` is authoritative for how work is planned, verified and committed. Read it in
full before writing or executing a plan.

## Map <!-- hp:context-map -->

| Path | Holds | Read when |
|------|-------|-----------|
| `plans/RULES.md` | Plan/tracker format, Rules A–E, project rules | Before any plan work — in full |
| `plans/SERIES.md` | Dependency chain, status, next free plan number, north star | Before creating or picking a plan |
| `plans/NN_example.md`, `plans/NN_example_tracker.md` | The skeletons to copy | Writing a plan |
| `plans/completed/`, `plans/abandoned/` | Closed plans (newest = current conventions) | Looking for precedent |
{{CONTEXT_MAP_ROWS}}

## Where Findings Go <!-- hp:context-routing -->

| You discovered… | Write it in | Shape |
|-----------------|-------------|-------|
{{ROUTING_ROWS}}
| How a library/API works, its signatures, its sharp edges | `<lib_name>.md` (create on first use) | Compact usage + signatures; no full source |
| An algorithm known to be the right/fast way to do something | `algo.md` (create on first use) | Name, formula/idea, when it applies, link |
| A design pattern worth reusing | `patterns.md` (create on first use) | Problem → pattern → where we used it |
| Progress, a deviation from a plan, a negative result | The active plan's tracker | A row or a Notes/Deviations line |
{{PARENT_CONTEXT}}

## Provenance <!-- hp:context-provenance -->

Tag every fact with how it is known, and **never upgrade a tag without doing the work**:

- `[SOURCE]` read in upstream source code (cite `path:line`) · `[DOC]` read in documentation ·
  `[REPO]` asserted by this repo's own code/config · `[LIVE YYYY-MM-DD]` observed on a running
  system.
- Pitfalls that actually bit are tagged `[OBSERVED YYYY-MM-DD]`, to tell them apart from hazards
  seeded from reading.

## House Style <!-- hp:context-style -->

- **Dense.** No fluff — but never compress so far that a detail is lost.
- **No full source code** and nothing private or trademarked. Short algorithm explanations,
  formulas, signatures and architecture ideas are fine.
- Dates are ISO, from `date +%F`.
- A file past ~40 KB is split: `<name>/` with an `index.md` and one file per topic.
- A file that turns out wrong or stale is **fixed in place**, with a dated note of what changed.
{{VENDOR_LINE}}

## Honesty

**Never claim something is recorded unless the bytes are in the file.** Re-read the file after
writing it. "I noted that in pitfalls.md" is a factual claim about the tree.
