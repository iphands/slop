# patches/ — our changes to other people's code

Everything we write for Mesa, the kernel, or anything else we rebuild lives here, under
git. Nothing we author goes in `vendor/`: that tree is extracted from source RPMs, is
gitignored, and gets blown away and re-extracted every time this box takes an RPM update.
If a change is not in this directory, it does not exist.

```
patches/
├── mesa/
│   ├── series              # optional: explicit order, one filename per line
│   └── 0001-something.patch
└── kernel/
    ├── series
    └── 0001-something.patch
```

`scripts/vendor-build.sh` reads these at build time and injects them into a **copy** of the
distro spec, so the pristine spec stays pristine and `git diff` shows the whole of our work.

## Order

If `series` exists it is authoritative — one filename per line, `#` comments allowed. If it
does not, patches apply in filename sort order. Add a `series` file as soon as two patches
touch the same file; relying on sort order across a rename is how a patch set silently
starts applying backwards.

## Generating a patch

Work in the prepped source tree, then export:

```bash
cd vendor/mesa/BUILD/mesa-*/          # after `rpmbuild -bp`, or after a full build
git init && git add -A && git commit -qm base   # if it is not already a git tree
# ... edit ...
git diff > /path/to/gpu/patches/mesa/0002-my-change.patch
```

The patch must apply with `-p1`, which is what both injection paths use.

## Rules

- **One logical change per patch**, with a real commit-message-style header explaining
  *why*. These are intended to become upstream submissions; a patch nobody can review is
  not a result.
- **A patch that does not apply is a hard build failure**, deliberately. `rpmbuild` stops
  on a failed `%patch`, and `vendor-build.sh` refuses to build a spec whose shape it does
  not recognise. Both exist because an unpatched build "measures" as a patch with no
  effect — indistinguishable from a change that genuinely does nothing, and the more
  likely explanation of the two. See `context/pitfalls.md`.
- **Always build the pristine baseline too** (`vendor-build.sh --no-patches`) and compare
  against that, not against the distro RPM. Same toolchain, same box, same flags — one
  variable at a time.
