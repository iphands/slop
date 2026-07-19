# Project slop

A place to house random AI assisted experiments. Cool stuff may migrate away from here into standalone projects.

## Git discipline

Applies to every sub-project here — `cache`, `qbots`, `qctrl`, and anything added later.

### Git history is APPEND-ONLY. Never rewrite it.

**Banned outright.** Do not run these unless the human explicitly asks, in that moment:

| Banned | Why |
|---|---|
| `git commit --amend` | replaces a commit that may already be public |
| `git rebase` (any form) | rewrites every commit it touches |
| `git reset --hard`, or any reset that drops a commit | discards history |
| `git push --force` / `--force-with-lease` | forces the rewrite onto everyone else |
| `git revert` | banned here too — corrections are hand-written, not machine-generated |
| deleting or moving a branch to hide commits | same thing by another name |

**A mistake in a commit — wrong content, wrong message, a claim that turned out false — is
fixed by a NEW commit that says what was wrong and corrects it.**

```bash
# WRONG                          # RIGHT
git commit --amend               <edit the files>
                                 git commit -m "fix(T6): record the findings the previous
                                                commit's message overclaimed"
```

A wrong commit left visible, with a correction after it, is honest history. A rewritten one
is a lie that also breaks everyone who pulled.

**This holds even when the commit is "obviously" unpushed.** Push state changes without you
seeing it. Checking it once at the top of a session and assuming it still holds hours later
is exactly how this goes wrong.

> **Incident, 2026-07-18 (`cache`).** A commit landed whose message claimed two doc files
> were updated when they were not. The human had already pushed it. It was "fixed" with
> `git commit --amend`, which diverged `main` from `origin/main` and forced the human to
> clean up with a force push. A follow-up commit would have corrected the record with zero
> blast radius.

### Chain edit-then-commit with `&&`

**A failed edit must never be followed by a commit that claims it succeeded.** This is the
root cause of the incident above — a heredoc script hit an assertion and wrote nothing,
while the `git commit` on the next line ran regardless.

```bash
edit_files.sh && git commit -m "..."     # RIGHT — commit cannot run if the edit fails
edit_files.sh
git commit -m "..."                      # WRONG — runs even when the edit blew up
```

### Verify a claim before putting it in a commit message

If the message says a file was updated, re-read that file first. A commit message is a
factual claim about the tree, and the same honesty rule applies to it as to anything else
you say.

### Also

- **NEVER push.** The human pushes after review. *(Global rule, `~/.claude/CLAUDE.md`.)*
- No co-author trailers unless asked.
- Small, frequent commits — one logical change each.

## Resources

### ./vendor

Here we will store clones of project source that we can use to read about and gain deep understandings about a third-party dependencies implementation.

From vendor:
- Read this for very thorough understandings about how deps, libs, etc. work
  - Sometimes the source is better than the API doc
- Feel free to ALSO use the web for fast/quick API references and help
- Look at examples directories in vendor for concrete implementation ideas
- Understand what optimizations are done in one language to give ideas of how to do so in another language
  - ie: Project Foobar a C library uses <some_technique> methods to optimize <some_implementation> we can make similar optimizations in Rust

### ./context

Here we will store (for long term) context / hints / API facts we discover that help us build better software.

Dos:
- If we discover in vendor/ or on the internet that an API/Library is best for <some_task> we can compact the info and store that in `./context/<lib_name>.md`
- If we discover that a particular algorithm is known as a great way to highly optimize function/method/etc we compact this info and store that in `./context/aglo.md`
  - Example: https://en.wikipedia.org/wiki/Fast_inverse_square_root we can store information about fast square root in the algo.md
- If we discover great generic design patterns we can store those in `./context/patterns.md`
- If we discover library specific design patterns we can store those in `./context/<lib_name>.md`
- We can also store compact library use info (and function signatures) in `./context/<lib_name>.md` for super fast retrieval when developing

DONTS:
- Dont store private, trademarked, etc information
- Dont store full source code! Short algorithm explanations, math formulas, and architecture ides are all fine


NOTE: We want this informaiton to be dense! DONT INCLUDE FLUFF and try to compact it a bit BUT DONT compact so much we lose details!

#### ./context/pitfalls.md

As we work through bugs and issues in sub-projects IF we encounter pitfalls, errors, bugs, etc
!!ESPECIALLY if we take multiple tries to fix!! Make take some short notes about the issue in `./context/pitfalls.md`.
Use 200 words or so to describe the pitfall and how to avoid.
You can add some info about which sub-project we discovered it in.

Use a template like
```
# <pitfall_title#
<200_words_or_so_to_describe_issue>
<200_words_or_so_to_describe_how_to_avoid>
## Sources
- <sub_project_one>: <short_filename> (optionally: <function_name>)
- <sub_project_two>: <short_filename> (optionally: <function_name>)
```

#### ./context/high_level.md

In `./context/high_level.md` store very high level and short explanations about deps we use and differences or why they might be better than various alternatives

Feel free to build short pros/cons tables of competing libs! ITS okay to mark which ones we use in sub-folder / projects BUT KEEP THE MENTION short!
ie: `ash` used in <sub-project-one> and <sub-project-two>
