# Plan 14 — hands-plan v1 Migration

> **Status**: in-progress
> **Created**: 2026-10-04
> **Depends on**: N/A
> **Goal**: qctrl's plan system (RULES, SERIES, skeletons, `context/AGENTS.md`, root `AGENTS.md`) matches hands-plan v1, so a fresh `/hands-plan resync` reports no ADD/UPDATE/SHAPE items.
> **Agent**: implementation agent

---

> **Before writing any code, re-read `context/plans/RULES.md` in full.**
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Migrate qctrl's hand-copied legacy plan system to hands-plan v1 in place, in the
order the 2026-10-04 resync gave: rules first, gate before the authority clause, root files once
RULES is authoritative, stamps last.

**Deliverables**:
1. `context/plans/RULES.md` carrying every v1 core section. Existing letters stay: gate = Rule A,
   commit = Rule B. New letters: append-only = Rule B2, lifecycle = Rule C, harvest = Rule D,
   evidence = Rule E. Also Project Rules and Series-Scoped Rules sections.
2. A project gate that is the full pre-commit set: backend **and** frontend, blind spots, a
   check that the build actually loaded, and prerequisites.
3. New files: `context/plans/NN_example.md`, `context/plans/NN_example_tracker.md`,
   `context/AGENTS.md` (+ `context/CLAUDE.md` symlink), `context/distilled.md`.
4. SERIES v1 structure (header, next free number, north star, Currently Active, Abandoned,
   compaction footer). No existing row or narrative is rewritten.
5. Root `AGENTS.md` changes, each diff approved by the operator first:
   - the *Plans & Context* block
   - de-duplication of restated rules
   - dead references fixed
   - a `CLAUDE.md -> AGENTS.md` symlink
6. `hp:<id>` markers on every mapped heading, and hands-plan v1 stamps.

**Estimated effort**: Small–Medium (half day)

---

## Scope

**In**: `context/plans/RULES.md`, `context/plans/SERIES.md` (structure only),
`context/plans/NN_example*.md` (new), `context/AGENTS.md` + `context/CLAUDE.md` (new),
`context/distilled.md` (new), root `AGENTS.md` + `CLAUDE.md` (new symlink) — root edits only on
approval.
**Out**:
- Real plans and trackers, anything in `completed/`, knowledge-file *content*, SERIES rows
  and narrative.
- Any code, including the `e2e-test.js` fix (that is Plan 12 Follow-up 2).
- The parent `slop/CLAUDE.md`: suggest only.

If a step needs to cross this line, **stop** and record the blocker in the tracker instead of
expanding scope.

---

## Context

`/hands-plan status` and `/hands-plan resync` (2026-10-04) found qctrl's plan system is legacy
and unstamped:
- 13 plans run under a RULES copied from another project, whose examples talk about bone
  meshes and "Plans 60–67".
- No skeleton files: both RULES and the root file point at a missing `NN_example.md`.
- A gate that covers only `cargo build` + `cargo clippy`, while the root file demands
  `just be-all` and `just fe-build` before commits.
- No append-only, harvest or evidence rules in RULES.

Plan 12 just exercised all three of those rules from the parent `CLAUDE.md` and the skill
instead.

### Pre-Identified Bug/Issue

```bash
$ "$SKILL_DIR/bin/hp-scan" --brief . | grep -E '^(FILE|ROOTFILE|DEADREF)'   # 2026-10-04
FILE	context/plans/RULES.md	exists=y	stamp=none	link=-	bytes=5746
FILE	context/plans/SERIES.md	exists=y	stamp=none	link=-	bytes=2559
FILE	context/plans/NN_example.md	exists=n	stamp=none	link=-	bytes=0
FILE	context/plans/NN_example_tracker.md	exists=n	stamp=none	link=-	bytes=0
FILE	context/AGENTS.md	exists=n	stamp=none	link=-	bytes=0
FILE	context/CLAUDE.md	exists=n	stamp=none	link=-	bytes=0
ROOTFILE	AGENTS.md	link=-	block=none
DEADREF	AGENTS.md:66	context/distilled.md
DEADREF	AGENTS.md:148	context/plans/01_setup.md
DEADREF	AGENTS.md:60	context/plans/NN_example.md
DEADREF	context/plans/RULES.md:169	context/plans/NN_example.md
```

Resync summary (same day): `0 CONFLICT · 0 TIGHTENING · 8 ADD · 12 UPDATE · 2 SHAPE · 7 INFO`.
Probe misses per section are listed under each task below.

### Why migrate in place

- **Rejected: re-scaffold with `/hands-plan init`.**
  - It would drop project text that has no other home: Rule B's NOTE lines are the only
    place RULES demands fmt and tests.
  - Old plans cite rule letters, and a re-scaffold would re-letter them.
- **Rejected: one big commit.** One task per checklist group keeps every diff reviewable, and
  each task's Verify is the probe set for exactly that group.

### Decisions (operator, 2026-10-04)

| Decision | Choice |
|---|---|
| Commit format | `[qctrl][P<n>][T<n>][<topic>] <summary>`. Bookkeeping: `[qctrl][P<n>][plan\|revise\|close] …`. Outside a plan: `[qctrl][<topic>] …` |
| Where confirmed facts go | Create `context/distilled.md` (the root file already points there) |
| Rule B "user doc" | `DEPLOYMENT.md` (qctrl has no README) |
| Root files | In scope (T6); each diff shown and approved when it runs |

### Key Facts

| Fact | Value | How confirmed |
|---|---|---|
| Next free plan number | 14 (SERIES has no "Next free" line) | `hp-scan --brief .` → `NEXT computed=14 … series=missing`, 2026-10-04 |
| RULES legacy layout | Plan File Format 7, Tracker File Format 101, Rule A 131, Rule B 140, Content Style 157, Canonical Template 167, Completed Plans 177, Mandatory Header 191 | `hp-probe context/plans/RULES.md --toc`, 2026-10-04 |
| Rule letters in use | Only A and B. `## Completed Plans` has no letter | same; `grep 'Rule [A-Z]' RULES.md` |
| Old plans cite rules by heading? | No. One letter citation: `completed/12_…md:379` "Rule A equivalent for TS"; A stays A | `grep -o 'Rule [A-Z]…\|Mandatory Header\|Canonical Template\|Completed Plans' completed/*.md`, 2026-10-04 |
| Only home of the fmt + tests requirement | Rule B NOTE lines, RULES:151–153 | read RULES, 2026-10-04 |
| Root pre-commit demands | `just be-all` (= `cargo fmt --all`, clippy `-D warnings`, `RUSTFLAGS="-D warnings" cargo build --release`) before backend commits; `just fe-build` before frontend commits; tests `cargo test --all-features` / `npm test` | `AGENTS.md` §3 Code Quality; `justfile:72-76` |
| `just fe-test` / `npm run testall` are live operations | `testall` = lint + build + `node e2e-test.js`, which drives `cosmo.lan:3000` | 2026-10-03 incident; `context/pitfalls.md` |
| Parent git rule | `slop/CLAUDE.md:5` `## Git discipline`; relative path from `context/plans/` = `../../../CLAUDE.md`; from `context/` to the parent context = `../../context` | `grep -n`, `realpath --relative-to`, 2026-10-04 |
| qctrl has no root `CLAUDE.md` | Only `AGENTS.md`, so Claude Code doesn't auto-load qctrl's rules | `ls -la CLAUDE.md` → missing, 2026-10-04 |
| Knowledge files | `context/pitfalls.md` only | `hp-scan` `KNOWLEDGE`, 2026-10-04 |
| `vendor/` contents | `base.md`, `quakeiicom.html`, `yquake2/`. **No** `q2pro/` (root `AGENTS.md` cites `vendor/q2pro/src/`) | `ls vendor/`, 2026-10-04 |
| Templates and line ranges | skill v1: `templates/plans/RULES.md` (header 1–8, plan-gate 12–19, naming 23–36, plan-format 40–70, tracker-format 74–90, gate 98–117, commit 119–133, append-only 135–163, lifecycle 165–183, harvest 185–194, evidence 196–206, project-rules 210–215, series-rules 219–227, style 231–237, templates 241–246) | read the skill files, 2026-10-04 |

### Rejected Claims

- **"Existing trackers violate v1, which says a row never records its own commit hash."**
  Plan 12/13 rows cite hashes, but completed trackers are history and never edited. The rule
  binds new trackers only.
- **"SERIES `## Notes` restates naming, so delete it."** It's SERIES narrative, which the
  migration never touches. Only the `## Completed Plans` footer is structural.

---

## Step-by-Step Tasks

The project gate in every Verify below means RULES Rule A as it stands at that moment. Before
T2 lands, that's `cargo build`/`cargo clippy` plus Rule B's NOTEs (fmt, tests). From T2 on,
it's the new gate. These tasks are docs-only. Still, run the gate before each commit, as RULES
demands.

**⚠ Never `just fe-test` or `npm run testall` as part of a gate** (it drives the live server).

`P=$SKILL_DIR/bin/hp-probe`, `R=context/plans/RULES.md`.

### T1: Per-task rules: commit format, append-only, lifecycle, harvest, evidence

**File**: `context/plans/RULES.md`

**What to do**: under `## Per-Task Execution Rules`, after Rule A:

1. **Rule B** (`### Rule B — Commit at every task boundary OR MORE FREQUENLTY`, RULES:140–156):
   - Replace the body with template `RULES.md:121–133`.
   - Heading becomes `### Rule B — Commit at every task boundary (or more often)`.
   - `R_GATE` = Rule A, `USER_DOC` = `DEPLOYMENT.md`, `COMMIT_FORMAT` = the After
     block below.
   - Move the three `NOTE:` lines and the `**MANDITORY** bake…` line under
     `#### Project addendum`, verbatim.
2. **Rule B2 — Git history is append-only** (new): template `RULES.md:135–161` verbatim, then
   `> Full rule: [\`../../../CLAUDE.md\`](../../../CLAUDE.md) § Git discipline.`
3. **Rule C — Plan lifecycle: `completed/` and `abandoned/`**:
   - Delete the unlettered `## Completed Plans` section (RULES:177–190) and write template
     `RULES.md:167–183` here. Its substance (the `git mv` pair, "mark SERIES done") is a subset.
4. **Rule D — Harvest** and **Rule E — Evidence** (new): template `RULES.md:185–206` verbatim.
5. After the per-task rules, add `## Project Rules` (template `:210–215`, with
   `PROJECT_RULES` = `_None yet — add **P1** when a project-specific rule earns its place._`)
   and `## Series-Scoped Rules` (template `:219–227`).
6. Resolve every `R_*` with this letter map: A gate, B commit, B2 append-only, C lifecycle,
   D harvest, E evidence. Keep the template's `hp:<id>` heading markers.

**Before** (RULES:144–147):
```markdown
3. Commit message format: `task(TN): <short description>` where `TN` is the plan task number.
   - Example: `task(T1): fix bone mesh pre-translation placement`
   - Example: `task(T2): apply Y/Z coordinate flip to renderer`
```

**After** (Rule B item 3):
```markdown
3. Message format: `[qctrl][P<n>][T<n>][<topic>] <imperative summary>` — sub-project scope (slop
   is a monorepo), plan number (not zero-padded), task number, short area tag. Outside a plan:
   `[qctrl][<topic>] <summary>`. Plan bookkeeping replaces the task tag:
   `[qctrl][P<n>][plan] add <title>`, `[qctrl][P<n>][revise] …`,
   `[qctrl][P<n>][close] <title>: <outcome>`.
   - Example: `[qctrl][P12][T1][watchdog] add background sv_maplist drift re-sync loop`
   - Example: `[qctrl][P14][plan] add hands-plan v1 migration`
   Commits before 2026-10-04 use `task(TN): …` or conventional prefixes; history is never rewritten.
```

**Verify**:
```bash
$P $R --heading '^### Rule [A-Z0-9]+ — Commit' -- "COMMIT BEFORE MARKING ANY TASK COMPLETE" "end of every task" "One task per commit" "Tracker row in the same commit" "never git add -a" "pass before every commit||passes before every commit" "observable behavior" "Never push" "co-author" "baked in||bake commit reminders" "Project addendum"
$P $R -- "append-only" "git commit --amend" "git clean" "new commit" "looks unpushed" "Chain edit-then-commit" "factual claim" "Why this rule exists" "../../../CLAUDE.md"
$P $R --heading '^### Rule [A-Z0-9]+ — Plan lifecycle' -- "git mv context/plans/NN_name.md context/plans/completed/" "mkdir -p" "partially complete" "abandoned/" "stated reason||recorded reason" "Currently Active" "hypothesis, not a contract" "fix links"
$P $R -- "as you go" "unless the bytes are" "more than one attempt" "proves nothing" "premise is a claim" "Below the bar" "Inconclusive" "path:line" "_RULES.md" "copy from X" "Project Rules"
grep -n -E '^### Rule [A-Z0-9]+ —' $R      # → A, B, B2, C, D, E in that order
grep -n -E '\{\{|^## Completed Plans' $R    # → nothing
# + project gate (Rule A + Rule B NOTEs): cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features
```
Expected: every probe `HIT`; six lettered rules in order; no placeholders and no legacy
`## Completed Plans`.

**Commit**: `[qctrl][P14][T1][rules] add per-task rules B2–E and the new commit format` — *commit before marking done (Rule B), tracker row in the same commit.*

### T2: The verification gate: full pre-commit set, backend + frontend

**File**: `context/plans/RULES.md` (Rule A, RULES:131–139)

**What to do**:
1. Rename the heading to `### Rule A — The verification gate`. Ask the operator first; they
   may keep "Zero build errors and warnings". Under it, write the preamble from template
   `RULES.md:100–113`.
2. Add `#### Project gate`, with the operator's approval of the wording. Draft:

```markdown
#### Project gate

Run the gate for every component the task touched, in order. All of it runs before **every**
commit (Rule B). Unit tests cannot see what rcon does to a command on the server.

**Backend** (touched `crates/`, `Cargo.*`, `config*.yaml`):
1. `cargo fmt --all --check` → exit 0.
2. `cargo clippy --all-targets --all-features -- -D warnings` → exit 0.
3. `cargo test --all-targets --all-features` → every `test result: ok`, `0 failed`.
4. `RUSTFLAGS="-D warnings" cargo build --release` → exit 0 (= `just be-all` without its
   mutating `cargo fmt`). ⚠ It rebuilds `target/release/qctrl-api`: if `pgrep -a qctrl-api`
   shows the operator's API running from it, ask first.

**Frontend** (touched `frontend/`):
5. `cd frontend && npm run lint && npm run test && npm run build` → exit 0 each. `npm run test`
   is vitest only. **Never** `just fe-test`, `npm run testall` or `just fe-e2e` as a gate:
   `e2e-test.js` drives the live server (`context/pitfalls.md`).

**Blind spots → behavioral checks:**
- Anything that changes what qctrl **sends** (rcon commands, cvar pushes, the watchdog/rotator)
  is verified on the live server by **reading the result back**. A `set` that replies with
  usage looks like success: quote stripping hid for months that way.
- **The build actually loaded:** for a live check, the running `qctrl-api` must have started
  after the build (`ps -o lstart -p <pid>` vs. the binary mtime), and `/api/health` → `ok`.
- **Look at it:** a UI change is opened in a browser and looked at. A passing build proves
  nothing about the page.

**Prerequisites:** `just`; node per `frontend/.nvmrc` (22); live checks need the operator's API
and the server on noir.lan. **Baseline (<date>):** <result of running steps 1–5 once>.
```
3. Run steps 1–5 once on the current tree (step 4 per the ⚠) and write the result into the
   **Baseline** line.

**Verify**:
```bash
$P $R --heading '^### Rule [A-Z0-9]+ — (Zero build|Prove it|The verification)' -- "Reading the diff is not verification" "zero warnings" "Know what the gate cannot see||blind to" "actually loaded||actually load" "looked at" "Keep the gate current" "Never mark a task"
$P $R --heading '^#### Project gate' -- "cargo fmt --all --check" "-D warnings" "cargo test" "npm run test" "Never" "Blind spots" "actually loaded||started after the build" "Look at it" "Prerequisites" "Baseline"
# + the new project gate itself, steps 1–5 (this IS the baseline run)
```
Expected: every probe `HIT`; the baseline line records real results.

**Commit**: `[qctrl][P14][T2][gate] make the RULES gate the full pre-commit set (backend + frontend)` — *tracker row in the same commit.*

### T3: Plan/tracker format, header, naming, style, and the NN_example skeletons

**Files**: `context/plans/RULES.md`; new `context/plans/NN_example.md`,
`context/plans/NN_example_tracker.md`

**What to do**:
1. **Header** (RULES:1–5): template `RULES.md:2–8` (title, read-in-full line, **"This file
   wins"**, the addendum convention). The stamp line waits for T7.
2. **`## When Does a Change Need a Plan?`** (template `:12–19`).
   - `PLAN_GATE_ROWS` is worded by the operator. Draft rows:
     - "Anything that changes what qctrl sends to the live server (rcon commands, cvar pushes,
       watchdog/rotator/poller cadence)": **Yes.** Both incidents were rcon behavior no unit
       test sees.
     - "A change spanning `crates/` and `frontend/`": **Yes.**
     - "A new background task that acts without a browser open": **Yes.**
3. **Naming** (RULES:9–15) → template `:23–36`. This drops "two-digit zero-padded" and the
   foreign examples `65_modelview_skel_fix.md` / `15_1_worldmap…`.
4. **Plan format** (RULES:7–100 *and* `## Mandatory Header in Every New Plan`, RULES:191–198)
   → template `:40–70`, with `#### Project addendum`: "For large plans, group tasks into
   parallel waves with an explicit dependency matrix." (RULES:76).
   - Drift dropped: the embedded skeleton, "≥ 90% coverage", "`./bin/debug-image` confirms
     humanoid silhouette", `// old code`.
5. **Tracker format** (RULES:101–126) → template `:74–90`.
6. **Content Style** (RULES:157–166) → template `:231–237` (`CODE_LANG` = `rust`), with
   `#### Project addendum`: "Absolute paths preferred in doc sections; relative paths acceptable
   inside task code blocks."
7. **`## Canonical Template`** (RULES:167–176) → `## Templates & History`, template
   `:241–246`. This drops "Plans 60–67 are the most recent".
8. Create both skeletons from `$SKILL_DIR/templates/plans/`, **without** the line-1 stamp (T7
   adds it). Fill:
   - `CODE_LANG` = rust
   - `VERIFY_CMD` = `cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features   # + frontend gate if frontend/ touched (Rule A)`
   - `COMMIT_EXAMPLE` = `` `[qctrl][P<n>][T1][<topic>] <short description>` ``
   - `COMMIT_FORMAT_SHORT` = `[qctrl][P<n>][T<n>][<topic>] <summary>`
   - Letters: R_GATE A, R_COMMIT B, R_LIFECYCLE C, R_HARVEST D, R_EVIDENCE E
   - Author-time placeholders (`NN`, the title and date slots) **stay**.

**Verify**:
```bash
$P $R --lines 1-10 -- "This file wins" "Project addendum"
$P $R -- "| change | plan? |" "When in doubt, write the plan" "Never renumber" "at least two digits" "unused across" "Must contain" "Revised" "the command that exercises" "Expected observation" "verbatim output" "How confirmed" "Rejected Claims" "instead of expanding scope" "exit gate" "Never delete one" "only against evidence" "N/A —" "re-read context/plans/RULES.md in full" "skipped | invalid" "carry a reason" "negative and inconclusive" "Deviations" "evidence (required when" "own commit hash" "Follow-ups" "When the first task starts" "date +%F" "file:line||path:line" "NN_example_tracker.md" "newest first" "dependency matrix"
grep -n -E 'humanoid|bone mesh|65_modelview|15_1_world|60–67|coverage|\{\{' $R context/plans/NN_example*.md   # → nothing
"$SKILL_DIR/bin/hp-scan" --brief . | grep -E 'DEADREF.*RULES|PLACEHOLDER|LEFTOVER'   # → nothing
# + project gate (Rule A)
```
Expected: every probe `HIT`; no foreign text or `{{…}}`; the RULES:169 dead reference is gone.

**Commit**: `[qctrl][P14][T3][rules] adopt the v1 plan/tracker format; add NN_example skeletons` — *tracker row in the same commit.*

### T4: `context/AGENTS.md` routing, its `CLAUDE.md` link, and `distilled.md`

**Files**: new `context/AGENTS.md`, `context/CLAUDE.md -> AGENTS.md` (symlink; the root has a
lone real `AGENTS.md`), new `context/distilled.md`

**What to do**: copy `$SKILL_DIR/templates/context/AGENTS.md` without the stamp (T7 adds it)
and fill it:
- `PROJECT_NAME` = qctrl
- `CONTEXT_MAP_ROWS`:
  - `distilled.md`: confirmed facts, provenance-tagged
  - `pitfalls.md`: bugs and gotchas
  - `../../context/`: the slop-wide knowledge base (`pitfalls.md`, `high_level.md`,
    `algo.md`, `patterns.md`, `<lib>.md`)
- `ROUTING_ROWS`:
  - confirmed fact (protocol, tool behavior, e.g. RCON packet structure, `dmflags` bitmasks —
    carried over from root §2) → `distilled.md`
  - bug/gotcha, especially a multi-attempt fix → `pitfalls.md`
  - dependency choice → `../../context/high_level.md`
- `PARENT_CONTEXT`: the cross-cutting row pointing at `../../context/pitfalls.md`.
- `PROVENANCE`: the default tag set.
- `VENDOR_LINE`: the vendor line (`vendor/` exists).

`context/distilled.md`: copy `$SKILL_DIR/templates/context/distilled.md` with
`PROJECT_NAME` = qctrl. Header only: no facts are invented, and harvesting old findings
into it is not this plan's job.

**Verify**:
```bash
$P context/AGENTS.md -- "authoritative" "unless the bytes are" "never upgrade a tag" "40 KB" "No full source" "distilled.md" "pitfalls.md" "../../context"
for p in plans/RULES.md plans/SERIES.md plans/NN_example.md plans/NN_example_tracker.md plans/completed distilled.md pitfalls.md ../../context/pitfalls.md ../../context/high_level.md; do test -e context/$p || echo "MISSING $p"; done   # → nothing
readlink context/CLAUDE.md                                    # → AGENTS.md
"$SKILL_DIR/bin/hp-scan" --brief . | grep -E 'DEADREF.*distilled|PLACEHOLDER'   # → nothing
# + project gate (Rule A)
```
Expected: every probe `HIT`; every mapped path exists; the `AGENTS.md:66` dead reference is
gone.

**Commit**: `[qctrl][P14][T4][context] add context/AGENTS.md routing and distilled.md` — *tracker row in the same commit.*

### T5: SERIES v1 structure

**File**: `context/plans/SERIES.md`. Structure only; **no row or narrative changes**.

**What to do**:
1. Header: template `SERIES.md:4–9` under the existing title. That's the "update whenever a
   plan is added, starts, completes or is abandoned" trigger, the status list, and the
   hypothesis line (`R_LIFECYCLE` = Rule C).
2. Confirm the **Next free plan number** line (added when this plan was registered) still
   equals `hp-scan` `NEXT computed=`.
3. `> **North star …**` + ordering principle (template `:13–15`), worded by the operator.
   Draft: "qctrl keeps an unattended Q2 server healthy: rotation, guards and status run
   headless; the UI is a convenience, not a dependency."
4. `## Currently Active` (template `:17–23`), listing Plan 14 as in progress.
5. `## Abandoned / Superseded` (template `:53–59`), with an empty table.
6. Replace the `## Completed Plans` footer with template `:65–69` (`SERIES_ARCHIVE` note). Do
   **not** compact anything.

**Verify**:
```bash
$P context/plans/SERIES.md -- "Update it whenever a plan is added||Update this file whenever" "hypothesis" "Next free plan number" "North star" "Currently Active" "| reason |" "SERIES_ARCHIVE"
git diff -U0 context/plans/SERIES.md | grep -E '^-\|'     # → nothing (no existing table row removed or edited)
# + project gate (Rule A)
```
Expected: every probe `HIT`; no existing table row touched.

**Commit**: `[qctrl][P14][T5][series] add the v1 SERIES structure` — *tracker row in the same commit.*

### T6: Root `AGENTS.md` (each diff approved) and the `CLAUDE.md` link

**Files**: root `AGENTS.md`; new root `CLAUDE.md -> AGENTS.md`

**What to do**: build each change as a diff, **show it, and apply only on the operator's
approval**. A declined item is recorded in the tracker, not forced.
1. **Block** (`$SKILL_DIR/templates/root/agents_block.md` snippet 2): after
   `### 4. Tooling & Scripts`, before `## Domain Knowledge`.
   - Fill R_GATE = Rule A and `COMMIT_FORMAT_SHORT` = `[qctrl][P<n>][T<n>][<topic>] <summary>`.
   - Add the sub-project line: "This is a sub-project of `slop`: `../CLAUDE.md` (git
     discipline, shared conventions) also applies."
2. **Tree** (snippet 1): merge `plans/` sub-lines (RULES, SERIES, `NN_example*`, `completed/`,
   `abandoned/`) under the existing `context/` lines. `high_level.md` becomes a parent pointer.
3. **De-dup** (snippet 3):
   - §1 Planning → a one-line pointer to RULES.
   - §2 Knowledge Management → "read `context/AGENTS.md`" (its unique rows moved there in T4).
   - §3 "Commits" (still says `task(TN)`) → point to Rule B.
   - §3 "Build Verification" → point to the Rule A gate (absorbed in T2).
4. **Stale**: `## Getting Started` (create Plan 01, `01_setup.md`) and `## Status` (Phase:
   Planning / Scaffolding) → "Current state lives in `context/plans/SERIES.md`."
5. **Dead reference**: `vendor/q2pro/src/` → `vendor/yquake2/src/`. **Flag only** (the
   operator's wording): "q2pro" in Project Goal and Domain Knowledge; the live server reports
   yquake2 8.70.
6. `ln -s AGENTS.md CLAUDE.md` in the qctrl root.
7. **Suggest only**: a one-line pointer in `slop/CLAUDE.md`:
   "`qctrl/` — has its own plan system: `qctrl/context/plans/`." This is outside the project.

**Verify**:
```bash
$P AGENTS.md -- "hands-plan:begin v1" "hands-plan:end" "context/plans/RULES.md is authoritative||RULES.md is authoritative" "../CLAUDE.md"
"$SKILL_DIR/bin/hp-scan" --brief . | grep -E '^(ROOTFILE|DEADREF)'   # → ROOTFILE … block=v1, no DEADREF
readlink CLAUDE.md                                              # → AGENTS.md
grep -n -E 'task\(TN\)|01_setup|Planning / Scaffolding|q2pro/src' AGENTS.md   # → nothing (unless the operator declined that item)
# + project gate (Rule A)
```
Expected: block present, no dead references, symlink in place. Any declined item is listed in
the tracker.

**Commit**: `[qctrl][P14][T6][root] add the Plans & Context block, de-dup AGENTS.md, link CLAUDE.md` — *tracker row in the same commit.*

### T7: Section markers and hands-plan v1 stamps

**Files**: `context/plans/RULES.md`, `context/plans/SERIES.md`,
`context/plans/NN_example.md`, `context/plans/NN_example_tracker.md`, `context/AGENTS.md`

**What to do**:
1. Add the heading marker — an HTML comment holding `hp:<id>`, exactly as the templates write
   it at the end of the heading line — to **every** mapped heading that lacks one, including sections no
   task touched:
   - RULES: plan-gate, naming, plan-format, tracker-format, gate, gate-project, commit,
     append-only, lifecycle, harvest, evidence, project-rules, series-rules, style, templates
   - SERIES: next-free, north-star, active, plans (on the existing `## Plan Status` table
     heading), abandoned
   - `context/AGENTS.md`: context-map, context-routing, context-provenance, context-style
2. Put the stamp line (the templates' line 1: `hands-plan`, `v1`, and `date +%F`) on line 1 of the five files. The root block's
   `hands-plan:begin v1` marker came with T6.
3. Re-run the full resync probe set (`$SKILL_DIR/reference/sections.md`) by `--section <id>`.

**Verify**:
```bash
"$SKILL_DIR/bin/hp-scan" --brief . | grep -E '^(FILE|ROOTFILE|MARKER|DEADREF|PLACEHOLDER|LEFTOVER)'   # → five FILE stamp=v1, ROOTFILE block=v1, no DEADREF/PLACEHOLDER/LEFTOVER
for id in plan-gate naming plan-format tracker-format gate gate-project commit append-only lifecycle harvest evidence project-rules series-rules style templates; do $P $R --section $id -- "hp:$id" >/dev/null || echo "NO MARKER $id"; done   # → nothing
# then /hands-plan resync → Summary: 0 ADD · 0 UPDATE · 0 SHAPE (INFO only)
# + project gate (Rule A)
```
Expected: every file stamped `v1`, every id found by its marker, and resync clean apart from INFO.

**Commit**: `[qctrl][P14][T7][stamp] mark sections and stamp hands-plan v1` — *tracker row in the same commit.*

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `context/plans/RULES.md` | per-task rules B2–E + commit format (T1), gate (T2), format/header/naming/style (T3), markers + stamp (T7) | P0 |
| `context/plans/NN_example.md` | *new* — plan skeleton (T3) | P0 |
| `context/plans/NN_example_tracker.md` | *new* — tracker skeleton (T3) | P0 |
| `context/AGENTS.md` | *new* — map, routing, provenance (T4) | P0 |
| `context/CLAUDE.md` | *new* — symlink → `AGENTS.md` (T4) | P1 |
| `context/distilled.md` | *new* — confirmed-facts file, header only (T4) | P1 |
| `context/plans/SERIES.md` | v1 structure (T5); Plan 14 row + Next free line at registration | P0 |
| `AGENTS.md` | block, tree, de-dup, stale sections, dead refs (T6, on approval) | P1 |
| `CLAUDE.md` | *new* — root symlink → `AGENTS.md` (T6, on approval) | P1 |

Priority values: `P0` = blocking, `P1` = important, `P2` = nice-to-have.

---

## Open Questions / Risks

1. **Risk: T2's gate step 4 rebuilds the binary the operator's API runs from.** *Mitigation*:
   check `pgrep -a qctrl-api` first. If it's running, ask before step 4. If the operator
   declines, record step 4 as not run in the baseline line, never as passed.
2. **Risk: between T1 and T6, root `AGENTS.md` still says `task(TN)` and "just be-all", while
   RULES says otherwise.** *Mitigation*: T3 lands "This file wins", and qctrl's root file isn't
   auto-loaded until T6 adds `CLAUDE.md`. Keep T1–T6 back to back.
3. **Question: the plan-gate rows (T3) and the north star (T5) are the operator's words.** *How
   we'll settle it*: show the drafts in those tasks and use the operator's wording. A declined
   draft leaves only the template's default row or line, noted in the tracker.
4. **Risk: heading renames.** Rule A, Rule B, `Completed Plans` → Rule C, `Canonical Template`
   → `Templates & History`; `Mandatory Header` merges away. *Mitigation*: confirmed
   2026-10-04 that old plans cite only "Rule A", by letter, and letters never change.
5. **Risk: the started-state flip names *Currently Active*, which doesn't exist until T5.**
   *Mitigation*: T1 sets the plan and its SERIES row to `in-progress`; T5 adds the section and
   lists Plan 14 there.
6. **Question: the root edits (T6) may be partly declined.** *How we'll settle it*: per-item
   approval. A declined item is recorded in the tracker, and T7 stamps whatever exists. A
   missing block stays a resync INFO/ADD, not a failure of this plan.

---

## Verification Checklist

- [x] T1: the T1 probe set all `HIT`; `grep '^### Rule'` lists A, B, B2, C, D, E in order; no `{{`, no legacy `## Completed Plans`.
- [x] T2: gate probes and gate-project shape probes all `HIT`; the Baseline line records a real run of steps 1–5 (or step 4 explicitly "not run — <reason>").
- [ ] T3: format/header/naming/style/templates probes all `HIT`; no foreign text or `{{`; `hp-scan` shows no `RULES.md:169` dead reference.
- [ ] T4: `context/AGENTS.md` probes `HIT`; every mapped path exists; `readlink context/CLAUDE.md` → `AGENTS.md`; no `AGENTS.md:66` dead reference.
- [ ] T5: SERIES probes all `HIT`; `git diff -U0 SERIES.md | grep '^-|'` empty.
- [ ] T6: `hp-scan` `ROOTFILE … block=v1`, no `DEADREF`; `readlink CLAUDE.md` → `AGENTS.md`; declined items listed in the tracker.
- [ ] T7: five `FILE … stamp=v1`; every id found by `--section`; `/hands-plan resync` reports 0 ADD / 0 UPDATE / 0 SHAPE.
- [ ] All: the project gate (Rule A) passes on the final commit.
- [ ] All: findings harvested into `context/` (Rule D) — bytes on disk, re-read.
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C).
