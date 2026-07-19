# Plans — Rules & Conventions

> Read this before writing any plan file or tracker file in `context/plans/`.

> **pkgcache note:** these rules are the `slop` house format, shared with
> [`../qbots`](../../../qbots/context/plans/RULES.md) and
> [`../qctrl`](../../../qctrl/context/plans/RULES.md). **Rule A is different here** —
> this project has no compiler. Read it carefully; "it looks right" is never done.

---

## When Does a Change Need a Plan?

Not everything does. This project is small.

| Change | Plan? |
|---|---|
| A TTL value, a comment, a README fix, a new env knob | **No.** Just do it, verify (Rule A), commit. |
| A new upstream/route block, a new `scripts/fix-<distro>` | **Yes.** |
| Cache-key, cache-zone, or `proxy_pass`/`rewrite` shape changes | **Yes** — highest-risk area in the repo. |
| A second container/service, TLS, auth, prefetching, stats | **Yes.** |

When in doubt, write the plan. It is cheap.

---

## Plan File Format

### Naming

- `NN_name.md` — two-digit zero-padded number, snake_case name (e.g. `02_opensuse_zypper_route.md`)
- Sub-plans: `NN_N_name.md` (e.g. `02_1_opensuse_client_fixer.md`)
- Trackers: `NN_name_tracker.md` — always paired with the plan
- `SERIES.md` — master dependency chain across all plans (no number)

### Metadata Block

Every plan file must open with a title and this metadata block:

```markdown
# Plan NN — [Title]

> **Status**: pending | in-progress | done
> **Created**: YYYY-MM-DD
> **Depends on**: Plan N | N/A
> **Goal**: One-sentence deliverable description.
> **Agent**: implementation agent (ralph-loop) | sub-agent | etc.

---
```

### Required Sections (in this order)

#### `## TL;DR`

```markdown
**What**: One sentence describing what is being done.

**Deliverables**:
1. Concrete output one
2. Concrete output two

**Estimated effort**: Small (2 h) | Small–Medium (half day) | Medium (1 day) | Large (3 days)
```

#### `## Context`

Background, rationale, prior findings, and decisions made. Use H3 subsections:

- `### Pre-Identified Bug/Issue` — confirmed bugs documented before work starts
- `### Why [Approach]` — justification for a design choice
- `### Key Facts` — upstream repo layout, path shapes, TTL semantics, tool behavior

**For this project, `### Key Facts` should record the upstream URL shape** you are
routing to (a real, fetched example URL for both a metadata file and a package file).
Guessed path shapes are the #1 source of broken route blocks — see
`context/pitfalls.md`.

#### `## Step-by-Step Tasks`

One H3 per task, labeled `T1`, `T2`, etc.:

````markdown
### T1: [Task title]

**File**: `proxy/conf.d/pkgcache.conf`

**What to do**: Detailed instructions.

**Before**:
```nginx
# old config
```

**After**:
```nginx
# corrected config
```

**Verify**: the exact command(s) that prove it, and the expected output.
````

The **Verify** line is mandatory on every task in this project. There is no test
suite to fall back on.

#### `## Critical Files`

| File | Change | Priority |
|------|--------|----------|
| `proxy/conf.d/pkgcache.conf` | Description of change | P0 |

Priority values: `P0` = blocking, `P1` = important, `P2` = nice-to-have.

#### `## Open Questions / Risks`

Numbered list. Each point names the risk and suggests a mitigation.

#### `## Verification Checklist`

One checkbox per task, each a **testable assertion with an observable output**:

```markdown
- [ ] T1: `curl -sI $CACHE/opensuse/.../repomd.xml` returns 200, `X-Cache-Status: MISS`
- [ ] T2: same URL again returns `X-Cache-Status: HIT`
- [ ] T3: `zypper ref` on a real client succeeds with the cache repo enabled
- [ ] T4: `./scripts/fix-opensuse --revert` restores the original repo files
```

---

## Tracker File Format

Every non-trivial plan gets a paired tracker: `NN_name_tracker.md`.

```markdown
# [Plan Title] — Tracker

## Overview
- Status: N% complete
- Start date: YYYY-MM-DD
- Test endpoint: http://localhost:8080 (or the live cache, noted explicitly)

## Resume Instructions
[How to pick up work if interrupted — which files, which commands, what state
 the test container is expected to be in]

## Progress

| # | Task | File | Status | Notes |
|---|------|------|--------|-------|
| 1 | T1: ... | `proxy/conf.d/pkgcache.conf` | pending | |

## Notes / Deviations
[Anything the plan asserted that turned out to be wrong. Be blunt — a wrong Key
 Fact recorded honestly here is worth more than a clean-looking tracker.]
```

**Status values**: `pending` | `in-progress` | `done` | `blocked` | `skipped`

---

## Per-Task Execution Rules

These rules apply to **every task** (T1, T2, …) during implementation. They are not optional.

### Rule A — Prove it runs and caches (this project's compiler substitute)

There is **no build step that can catch a mistake here.** `./build` happily produces an
image containing a broken `proxy/nginx.conf`; the failure only appears when the container
crash-loops, or worse, when it starts fine and silently serves 404s or misses.

**Container engine: use `docker` to verify.** This dev machine has docker and its rootless
podman is broken (`podman system migrate` needed). The live host `noir.lan` is the reverse
— podman, no docker. So verify locally with `RUNTIME=docker`, and leave the scripts'
podman-preferred auto-detection alone; that is what makes them work unchanged in
production. Substitute `podman` in these commands when running on the live host. One
consequence: the `--userns=keep-id` path only exists under rootless podman and therefore
**cannot be verified here** — treat it as unverified until the first live deploy.

After completing each task, in order:

1. **Config parses:**
   `RUNTIME=docker ./build && docker run --rm --entrypoint nginx <IMAGE>:latest -t`
   → must print `syntax is ok` / `test is successful`.
   ⚠️ **`nginx -t` passing means almost nothing here.** It catches syntax and a few
   directive-context errors (e.g. a URI on `proxy_pass` in a regex location) but is blind
   to every semantic failure that matters: a wrong upstream path, a lost `rewrite`, a
   duplicate `:8080` server block shadowing ours, or an `access_log` that silently writes
   nothing because the server has no `root`. **Never stop at step 1.**
2. **Container stays up:**
   `RUNTIME=docker PORT=8080 CACHE_DIR=/tmp/pkgcache-test ./run && sleep 2 && docker ps`
   → the container is `Up`, not restarting. Check `docker logs pkgcache` for
   `emerg`/`error` lines.
3. **Health + routes:** `curl -f http://localhost:8080/healthz` → `ok`.
4. **Cache actually caches** — for every route the task touched, hit **both** a
   metadata URL and a package URL, **twice**:
   ```bash
   curl -sI "$U" | grep -i x-cache-status   # expect MISS (first)
   curl -sI "$U" | grep -i x-cache-status   # expect HIT  (second)
   ```
   A `200` with no `HIT` on the second request is a **failure**, not a curiosity.
5. **Shell tasks:** `shellcheck build publish run scripts/*` → clean.
6. **Client-fixer tasks:** run it *and* `--revert` it on a real client (or a
   throwaway container of that distro), and confirm the package manager's refresh
   command succeeds in both states.

**Never mark a task `done` on unverified config.** Reading the diff is not verification.

### Rule B — Commit at every task boundary OR MORE FREQUENTLY

**CRITICAL: YOU MUST COMMIT AT EVERY TASK COMPLETION. DO NOT WAIT.**

1. Make a commit **at the end of every task** — no exceptions! You can make smaller
   commits too. **DO NOT WAIT UNTIL A FULL PLAN IS COMPLETE TO COMMIT** (commit early
   and often).
2. Intermediate commits within a task are encouraged for logical checkpoints.
3. Commit message format: `task(TN): <short description>` where `TN` is the plan task
   number.
   - Example: `task(T1): add /opensuse route with metadata/rpm TTL split`
   - Example: `task(T2): scripts/fix-opensuse with --revert`
4. The commit for a task must include only the changes for that task — do not batch
   multiple tasks into one commit unless they are inseparable.
5. **YOU MUST COMMIT BEFORE MARKING ANY TASK COMPLETE.** If you haven't committed, you
   haven't finished.

NOTE: Rule A verification must pass **before** every commit — including `shellcheck`.
NOTE: If the task changed observable behavior, **`README.md` is updated in the same
commit.** It is the user-facing contract.
NOTE: **Never push.** The human pushes after review. No co-author trailers unless asked.

**MANDATORY** bake commit reminders into the plan TODO / Task lists!

### Rule B2 — Git history is APPEND-ONLY. Fix forward, never rewrite.

**CRITICAL: NEVER REWRITE A COMMIT. NOT EVEN THE ONE YOU JUST MADE.**

Banned outright, unless the human explicitly asks in that moment:

```
git commit --amend        git rebase (any form)        git revert
git reset --hard          git push --force[-with-lease]
```

1. A mistake in a commit — wrong content, wrong message, a claim that turned out false —
   is fixed by a **NEW commit** that states what was wrong and corrects it:
   ```bash
   # WRONG                     # RIGHT
   git commit --amend          <edit the files>
                               git commit -m "fix(TN): record the findings the previous
                                              commit's message overclaimed"
   ```
2. **This applies even when the commit looks unpushed.** Push state changes without you
   seeing it. Verifying it once at the start of a session and assuming it still holds is
   exactly how this fails.
3. **Chain edit-then-commit with `&&`.** A failed edit must never be followed by a commit
   claiming it succeeded:
   ```bash
   edit_files.sh && git commit -m "..."   # RIGHT
   edit_files.sh                          # WRONG -- the commit below runs anyway
   git commit -m "..."
   ```
4. **A commit message is a factual claim about the tree.** If it says a file was updated,
   re-read that file before writing the message.

> **Why this rule exists (2026-07-18, Plan 02 T6).** A heredoc that was supposed to edit
> `distilled.md` and `pitfalls.md` hit an assertion and wrote nothing, but the unchained
> `git commit` on the next line ran anyway — producing a commit whose message claimed
> updates it did not contain. The human had already pushed it. "Fixing" it with
> `git commit --amend` diverged `main` from `origin/main` and forced the human to recover
> with a force push. A follow-up commit would have cost nothing.

Full rule: [`../../../CLAUDE.md`](../../../CLAUDE.md) § Git discipline (applies to all slop
projects).

### Rule C — Move completed plans to `completed/`

**CRITICAL: WHEN A PLAN IS 100% COMPLETE, MOVE IT TO `completed/` IMMEDIATELY.**

1. When a plan and its tracker reach 100% completion (all tasks done, all verification
   passed):
   ```bash
   git mv context/plans/NN_name.md context/plans/completed/NN_name.md
   git mv context/plans/NN_name_tracker.md context/plans/completed/NN_name_tracker.md
   ```
2. Update `SERIES.md` to mark the plan **done** if not already marked.
3. **DO NOT LEAVE COMPLETED PLANS IN THE ACTIVE `context/plans/` DIRECTORY.**
4. If a plan is partially complete (some tasks done, some pending), **DO NOT MOVE IT**.
5. **Before starting a new plan, verify that the previous plan is either moved to
   `completed/` or marked as deferred/blocked in SERIES.md.**

**FAILURE TO MOVE COMPLETED PLANS IS A VIOLATION OF THESE RULES.**

### Rule D — Harvest the knowledge before you close the plan

A plan is not done when the config works. It is done when what you learned is on disk:

1. New confirmed facts about an upstream repo layout, an nginx directive's real
   behavior, or a package manager's fetch pattern → **`context/distilled.md`**.
2. Anything that cost you more than one attempt → **`context/pitfalls.md`**, in the
   `# Title → Problem → Fix / How to avoid → Sources` template.
3. A library/tool comparison that informed a choice → **`context/high_level.md`**.

**Never claim a finding is recorded unless the bytes are actually in the file.**

---

## Content Style

- **Bold** for important terms; `code` for file names, variable names, commands.
- Dates always ISO format: `YYYY-MM-DD`.
- Absolute paths preferred in doc sections; relative paths acceptable inside task code blocks.
- Code blocks always carry a language specifier (` ```nginx `, ` ```bash `, ` ```ini `).
- Cross-reference other plans as "Plan N" or "Plan N T2".
- Real URLs in Key Facts, not placeholders. `<CACHE>` is fine for the cache host;
  upstream paths must be concrete.

---

## Canonical Template

Use `context/plans/NN_example.md` as the template for every new plan. Copy it, rename
it to `NN_name.md` (with the next zero-padded plan number from `SERIES.md`), and fill
in every section.

For historical context and real examples, browse `context/plans/completed/`.

---

## Mandatory Header in Every New Plan

Every plan file must include this reminder block immediately after the metadata block:

```markdown
> **Before writing any config, re-read `context/plans/RULES.md` in full** — especially
> Rule A (prove it runs and caches; there is no compiler here).
> For historical context, completed plans live in `context/plans/completed/`.
```
