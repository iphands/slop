# context/ — qctrl's living memory

You are reading inside `context/`, the project's knowledge base. Agents that skip it repeat
solved problems; agents that don't write to it leave traps for the next one. **Read the relevant
files before new work. Write what you learn as you go — in the same commit as the work.**

`plans/RULES.md` is authoritative for how work is planned, verified and committed. Read it in
full before writing or executing a plan.

## Map <!-- hp:context-map -->

| Path | Holds | Read when |
|------|-------|-----------|
| `plans/RULES.md` | Plan/tracker format; the per-task rules (verification gate, commits, append-only git, lifecycle, harvest, evidence); project rules | Before any plan work — in full |
| `plans/SERIES.md` | Dependency chain, status, next free plan number, north star | Before creating or picking a plan |
| `plans/NN_example.md`, `plans/NN_example_tracker.md` | The skeletons to copy | Writing a plan |
| `plans/completed/`, `plans/abandoned/` | Closed plans (newest = current conventions) | Looking for precedent |
| `distilled.md` | Confirmed facts about RCON, the Q2 server and our tools — provenance-tagged | Before touching protocol, cvar or server-facing code |
| `pitfalls.md` | Bugs and gotchas, especially multi-attempt fixes (rcon quoting, the live-server e2e trap, …) | Before any new work |
| `../../context/` | The slop-wide knowledge base: `pitfalls.md` (incl. the Q2 entries — empty `sv_maplist`, rcon quote stripping, intermission), `high_level.md`, `algo.md`, `patterns.md`, `<lib>.md` | When a topic isn't qctrl-specific |

## Where Findings Go <!-- hp:context-routing -->

| You discovered… | Write it in | Shape |
|-----------------|-------------|-------|
| A confirmed fact — protocol detail, cvar or server behavior, tool/library behavior (e.g. RCON packet structure, `dmflags` bitmasks) | `distilled.md` | An entry with a provenance tag, above `## Open Questions` |
| A bug, a gotcha, anything that took more than one attempt | `pitfalls.md` | `# Title`, the problem, how to avoid it, `## Sources` |
| A dependency choice (what we picked, what we rejected) | `../../context/high_level.md` | A short pros/cons table; mark it "used in qctrl" |
| Progress, a deviation from a plan, a negative result | The active plan's tracker | A row or a Notes/Deviations line |
| Anything that generalizes beyond this project | also `../../context/pitfalls.md` (or a `<lib>.md` / `high_level.md` there) | Same shape; keep project-specific detail here |

## Provenance <!-- hp:context-provenance -->

Tag every new fact with how it is known, and **never upgrade a tag without doing the work**:

- `[SOURCE]` (upstream code, cite `path:line`), `[DOC]` (documentation), `[REPO]` (this repo's own code/config), `[LIVE YYYY-MM-DD]` (observed on a running system)
- Pitfalls that actually bit are tagged `[OBSERVED YYYY-MM-DD]`, to tell them apart from hazards
  seeded from reading.

## House Style <!-- hp:context-style -->

- **Dense.** No fluff — but never compress so far that a detail is lost.
- **No full source code** and nothing private or trademarked. Short algorithm explanations,
  formulas, signatures and architecture ideas are fine.
- Dates are ISO, from `date +%F`.
- A file past ~40 KB is split: `<name>/` with an `index.md` and one file per topic.
- A file that turns out wrong or stale is **fixed in place**, with a dated note of what changed.
- `vendor/` holds third-party source clones: **read-only** reference; never write docs into it — distill into `context/`.

## Honesty

**Never claim something is recorded unless the bytes are in the file.** Re-read the file after
writing it. "I noted that in pitfalls.md" is a factual claim about the tree.
