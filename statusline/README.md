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

## Pitfall: `~/.claude/statusline/` is still an upstream clone

On this machine the live script was installed by cloning upstream directly, so
`~/.claude/statusline/` **is a git working tree with `origin` pointing at upstream** — and
our customization currently lives there as uncommitted local modifications.

Upstream's `INSTALL.md` documents updating with:

```bash
git -C ~/.claude/statusline pull     # <-- will fight the local edits
```

That pull will conflict with, or discard, the local changes. **This repo's copy is the
source of truth.** To update against a newer upstream: pull in a scratch clone, re-apply
the three changes above, verify, and re-vendor here — don't pull in place. Or run
`./install` from this repo, which overwrites the live script with the vendored copy.

## Scope

Only `statusline.sh` (macOS/Linux) is forked. Upstream's `statusline.ps1` (Windows) is
**not** included and carries none of these changes — Windows users should use upstream
directly.
