# statusline

A **fork** of [daniel3303/ClaudeCodeStatusLine](https://github.com/daniel3303/ClaudeCodeStatusLine),
customized to always show the working directory.

- **Upstream pin:** commit `5da9695`
- **License:** MIT — Copyright (c) 2025 Daniel Oliveira. See [`LICENSE`](LICENSE), retained
  verbatim. All the heavy lifting (token accounting, rate-limit windows, version cache,
  color theme) is upstream's work; the local delta is ~13 lines.

Rendered:

```
Opus 4.8 1M | ~/prog/slop/cache | cache@main | 42k/1m (4%) | effort: high | 5h 1% @23:10 | 7d 4% @Fri Jul 24, 04:00 | v2.1.214
```

## What was changed

All three changes are in the "Current working directory" block of `statusline.sh`.
Upstream showed only the directory *basename* (`cache`); we wanted the full path.

1. **The full path is its own segment**, sitting between the model and the repo segment.
   Upstream had no such segment at all.

2. **`$HOME` collapses to `~`** — and does so *boundary-aware*. Matching is anchored with
   a `case` on `"$HOME"` and `"$HOME"/*` rather than a substring replace, so
   `/home/iphands-other` renders in full instead of becoming the wrong `~-other`, and a
   `$HOME` occurring mid-path is never rewritten.

3. **The `basename@branch` segment is dropped outside a git repo** — separator included,
   so no orphaned `|` is left behind. Without this, `/tmp` rendered the redundant
   `| /tmp | tmp |`. Inside a repo it is unchanged, diffstat and all: `cache@main (+12 -3)`.

`git -C` still runs against the real `$cwd`, not the abbreviated display string, so branch
and diffstat detection are unaffected.

## Install

```bash
./install                 # copy statusline.sh into place + wire settings.json
./install --revert        # restore the settings.json backup, remove the script
```

Knobs: `DEST` (default `~/.claude/statusline`), `SETTINGS` (default
`${CLAUDE_CONFIG_DIR:-~/.claude}/settings.json`). Requires `jq` — both to merge
`settings.json` and at runtime, since the status line itself parses its stdin with it.

Restart Claude Code afterward; the status line is read at startup.

## How the live install is wired

On this machine `~/.claude/statusline` is a **symlink to this directory**:

```
~/.claude/settings.json  → "~/.claude/statusline/statusline.sh"
~/.claude/statusline     → /home/iphands/prog/slop/statusline   (symlink)
```

So the file that actually runs is this repo's tracked copy — edit it here and the change
is live immediately, with no install step and nothing to keep in sync. `settings.json`
needs no change if the repo moves; only the symlink does.

It was previously an upstream git clone with our edits sitting in it uncommitted, which
meant the `git -C ~/.claude/statusline pull` that upstream's `INSTALL.md` recommends would
have fought or discarded them. That clone is gone.

**Consequence:** there is no local upstream history to merge against anymore. To take a
newer upstream version, clone it fresh to a scratch directory, re-apply the three changes
above, verify, and copy the result here — see Verifying below.

**Consequence:** if this repo is moved or deleted, the symlink dangles and the status line
silently renders nothing (Claude Code does not report the error). Re-point the symlink.

## Verifying

The script reads a JSON blob on stdin, so it can be exercised directly — no Claude Code
restart needed:

```bash
for d in "$PWD" /tmp "$HOME" /home/iphands-other; do
  echo "{\"cwd\":\"$d\",\"model\":{\"display_name\":\"Opus 4.8 (1M context)\"}}" \
    | ./statusline.sh | sed 's/\x1b\[[0-9;]*m//g'
done
```

Expect the `basename@branch` segment only in the git-repo case, and `/home/iphands-other`
rendered in full rather than as `~-other`.

## Scope

Only `statusline.sh` (macOS/Linux) is forked. Upstream's `statusline.ps1` (Windows) is
**not** included and carries none of these changes — Windows users should use upstream
directly.
