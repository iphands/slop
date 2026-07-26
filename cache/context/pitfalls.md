# Pitfalls & Gotchas — pkgcache

Read before new work. Every bug/gotcha, **especially** multi-attempt fixes.
Template: `# Title → Problem → Fix / How to avoid → Sources`.

Cross-cutting, non-pkgcache-specific pitfalls also go up to `../../context/pitfalls.md`
per the slop convention. nginx/apt/dnf specifics stay here.

> **Seeding note:** most entries below are drawn from the design constraints encoded in
> this repo's config comments and README, plus documented nginx/apt/dnf behavior — they
> are *hazards the code was written to avoid*, not incidents observed in this project.
> When one actually bites, add the symptom verbatim and the date. Tag genuinely observed
> failures with `[OBSERVED YYYY-MM-DD]` so they can be told apart from the seeds.
>
> **Five entries are real observations, not seeds.** Three found 2026-07-18 while probing
> the Plan 02 stats log format against a live nginx: *"A variable in the `access_log` path
> silently logs NOTHING without a valid `root`"*, *"Logging `$uri` mis-files every
> rewritten request"*, and *"`proxy_cache_valid` is the lowest-priority TTL"* (which was
> live in this repo from day one). One is a **process** failure rather than an nginx one:
> *"A failed edit followed by an unchained commit produces a lying commit message"*. And
> one came from finally *looking* at the dashboard: *"A UI verified only by build, types
> and curl still ships visible bugs"*.

---

# A UI verified only by build, types and curl still ships visible bugs

## Problem

`[OBSERVED 2026-07-19]` The stats dashboard passed everything that could be
automated — `tsc -b`, `vite build`, 11 unit tests on the pure chart maths, and
curl checks against every endpoint (200s, correct `Content-Type`, ETag→304,
gzip, SPA fallback). It was never *looked at*. The first screenshot showed four
bugs, one of them a real data defect:

1. **Every sparkline was flat** while the KPI tiles beside them showed 90+ MiB.
   The 24h window spans **25** distinct hour buckets, so indexing forward from
   `now - 24h` put the current hour at index 24 of a 24-slot array and dropped
   it. All recent traffic — i.e. all of it — vanished from the sparklines.
2. **Top-metadata was a column of indistinguishable 64-character hashes**,
   because apt fetches indices via `by-hash/`, whose files are named after their
   own SHA. "Last path segment" is the obvious display rule and the wrong one.
3. A tile reusing the KPI component **labelled cache fullness "hit ratio"**.
4. An explanatory subtitle ("a low ratio is correct here") printed
   unconditionally, so it appeared next to a 100% ratio and read as a bug.

None of these could fail a type check, a build, or an endpoint assertion. Every
one is obvious in a single glance at the rendered page.

## Fix / How to avoid

**Look at the thing.** For any UI, a rendered screenshot is a distinct
verification step, not a nicety — it is the only check that covers "the numbers
are present and correct but the picture is wrong".

Specific traps worth carrying forward:

- **Time-bucket fencepost.** A rolling N-hour window touches **N+1** buckets.
  Anchor a series on the *current* bucket and count backwards; do not index
  forward from the start. Assert that recent traffic lands in the **last** slot.
- **A "last path segment" display rule** breaks on content-addressed paths.
  Check what it renders for `by-hash/`, and fall back to the enclosing directory.
- **A reused component's hardcoded label** becomes a lie at the second call
  site. If a component prints a fixed word next to a caller-supplied number,
  make the word a prop.
- **An unconditional explanation** is a claim. Gate it on the condition it
  explains.

## Sources
- pkgcache: `stats/crates/stats/src/snapshot.rs` (sparkline anchor),
  `stats/crates/ingest/src/pkgname.rs` (`display_name`)
- pkgcache: `stats/frontend/src/{components/Primitives,pages/Dashboard}.tsx`
- pkgcache: `docs/dashboard.jpg` — the screenshot in question
- pkgcache: fix commit `fa996c222`

---

# A failed edit followed by an unchained commit produces a lying commit message

## Problem

`[OBSERVED 2026-07-18]`, Plan 02 T6. A Python heredoc was used to update
`context/distilled.md` and `context/pitfalls.md`, then `git commit` ran on the next line:

```bash
python3 - <<'PY'
...
assert old_text in s        # <-- this failed
...
PY
git add -A . && git commit -q -F - <<'EOF'   # <-- ran anyway
```

The assertion failed, so **neither file was written** — but the commit still ran, with a
message stating that both had been updated. The commit landed containing only an unrelated
config change, carrying a message that was simply false.

Two things make this nastier than it looks:

1. **The script's failure is invisible in the commit.** `git commit` succeeds, output looks
   normal, and the lie is only discoverable by reading the diff against the message.
2. **The "obvious" fix makes it much worse.** The instinct is `git commit --amend`. The
   human had already pushed, so the amend diverged `main` from `origin/main` and forced
   them to recover with a force push. See `RULES.md` Rule B2 — history is append-only.

## Fix / How to avoid

**Chain the edit and the commit**, so the commit cannot run if the edit fails:

```bash
edit_files.sh && git commit -m "..."     # RIGHT
edit_files.sh                            # WRONG
git commit -m "..."
```

Note `set -e` does **not** save you here: the heredoc and the commit are separate top-level
commands in the tool call, not a single script.

**Verify the claim before writing it.** A commit message is a factual claim about the tree.
If it says a file was updated, `grep` that file first. `git show --stat HEAD` after
committing catches it in one line.

**When it happens anyway: fix forward.** A new commit that says *"the previous commit's
message overclaimed; here are the changes it promised"* is honest, costs nothing, and
cannot break anyone. Never amend.

## Sources
- pkgcache: `context/plans/RULES.md` (Rule B2), `../CLAUDE.md` (§ Git discipline)
- pkgcache: incident commits `c815c54e6` → `2c937c524`, 2026-07-18

---

# A nested regex `location` cannot inherit its parent's `proxy_pass` path remap

## Problem

The natural way to give package files a longer TTL is a regex location nested inside the
prefix location:

```nginx
location /fedora/ {
    proxy_pass https://dl.fedoraproject.org/pub/fedora/;   # remaps /fedora/ -> /pub/fedora/
    location ~* \.rpm$ {
        proxy_pass https://dl.fedoraproject.org/pub/fedora/;   # ✗ nginx REFUSES this
    }
}
```

nginx rejects a `proxy_pass` **with a URI part** inside a regex location at config-parse
time — **verified live 2026-07-18** on `nginxinc/nginx-unprivileged:1.27-alpine`:
`[emerg] "proxy_pass" cannot have URI part in location given by regular expression, or
inside named location, or inside "if" statement, or inside "limit_except" block`. Drop
the path to satisfy it and the request now goes upstream as `/fedora/…` instead of
`/pub/fedora/…` — **every `.rpm` 404s** while metadata keeps working, because metadata is
served by the parent location that still has its remap. The result looks like "the cache
half-works", which sends you hunting for a cache bug rather than a routing bug.

## Fix / How to avoid

Re-apply the remap by hand inside the regex location, then use a bare host:

```nginx
location ~* \.rpm$ {
    rewrite ^/fedora/(.*)$ /pub/fedora/$1 break;
    proxy_pass https://dl.fedoraproject.org;
    proxy_cache pkg;
    proxy_cache_valid 200 365d;
}
```

Debian needs **no** `rewrite` — client path `/debian/…` already equals the upstream path,
so a bare `proxy_pass http://deb.debian.org;` is right there and wrong for Fedora. The
two blocks look inconsistent on purpose.

**Rule:** whenever the client prefix ≠ the upstream prefix, every nested regex location
needs its own `rewrite … break`. **Always verify by fetching a real package file through
the cache** — a metadata-only test passes even when this is completely broken.

## Sources
- pkgcache: `proxy/conf.d/pkgcache.conf` (`/fedora/` and `/debian/` blocks)
- pkgcache: `context/distilled.md` (proxy_pass path-rewriting rule)

---

# Stale repo metadata → apt/dnf hash-mismatch failures

## Problem

Repo metadata (`InRelease`, `Release`, `Packages*`, `by-hash/…`, `repomd.xml`) is
regenerated upstream constantly and refers to package/index files by **content hash**.
Cache it aggressively — the intuitive "it's all just files, cache it for a year" move —
and clients start failing with:

```
Hash Sum mismatch / File has unexpected size          (apt)
Failed to download metadata for repo 'fedora-cache'   (dnf)
```

because the cached index names hashes that upstream has already rotated away. Worse, the
failure is **intermittent and per-client**, depending on who cached what when, so it reads
like a flaky network rather than a config decision.

## Fix / How to avoid

Keep the two-tier split that `proxy/conf.d/pkgcache.conf` is built around:

- metadata → `proxy_cache_valid 200 302 60s` + `proxy_cache_revalidate on`
  (revalidation makes the refresh a cheap `If-Modified-Since`, usually a `304`)
- package files (`*.deb`/`*.udeb`/`*.rpm`) → `proxy_cache_valid 200 365d` (immutable:
  a new version is a new filename, never new bytes at an old name)

**Never lengthen the metadata TTL to improve hit ratio.** Hit ratio is not the goal;
working clients are. If someone reports hash mismatches, suspect metadata TTL and
cache-key bugs *first*, before blaming upstream.

## Sources
- pkgcache: `proxy/conf.d/pkgcache.conf` (TTL split + header comment)
- pkgcache: `README.md` ("Caching split (correctness-critical)")

---

# `proxy_cache_valid` is the *lowest*-priority TTL — upstream `Cache-Control` overrides it

## Problem

`proxy_cache_valid 200 365d;` reads like an instruction. It is a **fallback**. nginx's TTL
precedence is `X-Accel-Expires` > `Cache-Control`/`Expires` (from upstream) >
`proxy_cache_valid`. `deb.debian.org` is a Fastly CDN and sends its own cache headers; if
those carry a short `max-age`, the "365 day" package cache is silently much shorter, and
packages get re-fetched far more often than the config implies. Nothing errors — you just
quietly don't get the cache you think you configured.

## Fix / How to avoid

Check what the upstream actually sends before trusting a `proxy_cache_valid`:

```bash
curl -sI http://deb.debian.org/debian/pool/main/c/cowsay/<file>.deb | grep -iE 'cache-control|expires'
```

If a short `max-age` is present, add `proxy_ignore_headers Cache-Control Expires;`
**inside the package sub-locations only** — never in the metadata locations, where
upstream freshness signals are exactly what you want.

**Status: CONFIRMED and fixed `[OBSERVED 2026-07-18]`.** This was live in this repo from
day one. `deb.debian.org` sends `Cache-Control: public, max-age=2592000`, so every `.deb`
was cached **30 days, not the configured 365**; Debian metadata got 120 s instead of 60 s.
Fedora sends no `Cache-Control` at all, so its TTLs were correct — which is precisely why
the bug was invisible. Fixed with `proxy_ignore_headers Cache-Control Expires;` in the
three package sub-locations only; metadata still defers to upstream on purpose.

**Measure it, don't read the config.** Retention is verifiable from the nginx cache file:
`valid_sec` is a little-endian `time_t` at byte offset 8 (after the 8-byte `version`),
holding an absolute unix expiry. Full numbers and the one-liner are in
`context/distilled.md`.

## Sources
- pkgcache: `context/distilled.md` (TTL precedence)
- nginx docs: `proxy_cache_valid`, `proxy_ignore_headers`

---

# A variable in the `access_log` path silently logs NOTHING without a valid `root`

## Problem

`access_log /path/access-$logdate.log fmt;` looks like it works: nginx starts, `nginx -t`
prints `test is successful`, the proxy serves and caches perfectly — and **the log file is
never created**. The only evidence is one line per request in the *error* log:

```text
[error] testing "/etc/nginx/html" existence failed (2: No such file or directory)
        while logging request
```

**Cause:** when an `access_log` path contains a variable, nginx tests the existence of the
server's **`root` directory** before each write and skips logging entirely if it is missing.
`nginxinc/nginx-unprivileged` ships **no `/etc/nginx/html`**, and a pure reverse-proxy
server block has no reason to declare a `root` — every location is a `proxy_pass` or a
`return`, so nothing ever serves a file from disk.

The failure is maximally deceptive: every other signal says healthy. `[OBSERVED 2026-07-18]`
while probing the Plan 02 stats log format — the dated log produced zero bytes until a
`root` was added, which was the **only** change required.

## Fix / How to avoid

Declare a `root` that exists, in any `server` block whose `access_log` path has a variable:

```nginx
server {
    listen 8080;
    root /tmp;      # exists in the image, writable by any uid; nothing is served from it
    …
}
```

`/tmp` is a deliberate choice — it exists, it is uid-agnostic, and no location ever reads
from it. Do not point it at the cache volume.

**General rule:** whenever an nginx directive path contains a variable, the parent
directory *and* the server's `root` must exist at request time; nginx will not create them
and will not fail loudly. Verify a variable-path log by asserting **the file exists and is
non-empty**, never by a clean `nginx -t`.

## Sources
- pkgcache: `proxy/conf.d/pkgcache.conf` (`root /tmp;`), `proxy/nginx.conf` (dated `access_log`)
- pkgcache: `context/plans/02_stats_foundation.md` T4a

---

# Logging `$uri` mis-files every rewritten request

## Problem

`$uri` is the **current** URI — after any `rewrite`. `$request_uri` is the **original**.
In this repo the `.rpm` sub-location rewrites `/fedora/…` → `/pub/fedora/…` (it has to; see
the first entry in this file). So a stats/analytics log using `$uri` records every Fedora
package under a path beginning `/pub/`, and any classifier taking "repo = first path
segment" files them under a repo named `pub`.

Metadata is unaffected — it is served by the parent location, which does no rewrite — so
**most of the log looks correct**. Measured `[OBSERVED 2026-07-18]`:

```text
metadata: uri=[/fedora/…/repomd.xml]           request_uri=[/fedora/…/repomd.xml]      agree
.rpm:     uri=[/pub/fedora/…/probe-1.0.rpm]    request_uri=[/fedora/…/probe-1.0.rpm]   DIVERGE
.deb:     uri=[/debian/…/probe.deb]            request_uri=[/debian/…/probe.deb]       agree
```

Three of four cases agree, so a casual test passes — and the one that diverges is `.rpm`,
which production logs show is the overwhelming majority of Fedora traffic. The result is a
dashboard that looks ~90% right while mis-bucketing exactly the high-value data.

## Fix / How to avoid

Log **`$request_uri`**. It is the original, pre-rewrite URI, it is what the cache key uses,
and it preserves percent-encoding (`usbmuxd-1.1.1%5e2025….rpm` stays encoded). There is no
case in this project where `$uri` is the right choice for a log.

To catch a regression, assert on real data rather than reading the config:
```bash
awk -F'\t' '$9 ~ /\.rpm/ {print $9}' access-*.log | head   # must start /fedora/
```

Related, same measurement session: **`$upstream_bytes_received` can exceed
`$body_bytes_sent`** on a MISS (5966 served vs 6499 upstream) because it counts response
headers. Any "bytes saved" figure computed as a whole-window `Σ served − Σ upstream` is
therefore biased downward by every MISS's header overhead and can go negative. Subtract
within the hit class only.

## Sources
- pkgcache: `proxy/nginx.conf` (`log_format stats`)
- pkgcache: `context/plans/02_stats_foundation.md` (Key Facts, "bytes saved")
- pkgcache: this file, first entry (the rewrite that causes it)

---

# A broken nginx config builds fine and only fails at runtime

## Problem

There is no compiler here. `./build` exits 0 with a totally invalid `proxy/nginx.conf` baked
into the image — the config is never parsed at build time. The failure surfaces as a
container that crash-loops (`docker ps` shows it restarting) or, more insidiously, as a
container that starts fine and serves 404s or permanent MISSes. "The diff looks right" has
no relationship to whether it works.

## Fix / How to avoid

`context/plans/RULES.md` **Rule A** is the substitute for a compiler. Minimum, every time:

```bash
# RUNTIME=docker on this dev machine; podman on noir.lan (see CLAUDE.md Critical Fact #7).
export RUNTIME=docker
./build && docker run --rm --entrypoint nginx iphands/pkgcache:latest -t   # syntax is ok
PORT=8080 CACHE_DIR=/tmp/pkgcache-test ./run && sleep 2
docker logs pkgcache | grep -iE 'emerg|error'                              # expect nothing
curl -f http://localhost:8080/healthz                                      # ok
curl -sI "$URL" | grep -i x-cache-status   # MISS
curl -sI "$URL" | grep -i x-cache-status   # HIT   <- the actual pass condition
```

Test **both** a metadata URL and a package URL for every route touched. A `200` with no
`HIT` on the second request is a failure, not a curiosity.

## Sources
- pkgcache: `context/plans/RULES.md` (Rule A)
- pkgcache: `proxy/Dockerfile`, `build`

---

# Rootless podman: host uid ≠ in-container uid, and the cache dir won't be writable

## Problem

`./run` passes `--user ${APP_UID}:${APP_GID}` and chowns `CACHE_DIR` to match. Under
**rootless podman** that host uid is *not* the uid the process actually has inside the
container (user-namespace mapping), so nginx fails to write the cache tree. Symptom:
permission-denied / `mkdir() failed` in the container logs right at startup, or a container
that starts but never caches anything.

## Fix / How to avoid

Under rootless podman, own `CACHE_DIR` as your **host** user and pass:

```bash
EXTRA_ARGS="--userns=keep-id:uid=1000,gid=1000" ./run
```

docker and rootful podman need no such flag. If nginx can't write at startup, check this
**before** suspecting the config.

Related: all writable paths must live under the mounted volume or `/tmp` (`pid
/tmp/nginx.pid`, every `*_temp_path` under `/var/cache/nginx`), and nginx's temp-dir
`mkdir()` is **single-level**, so those paths must sit one level under the volume root —
a deeper path fails on a fresh empty mount.

## Sources
- pkgcache: `run` (NOTE block), `README.md` ("Rootless podman"), `proxy/nginx.conf` (temp paths)

---

# The stock `default.conf` also binds :8080 and will shadow our server block

## Problem

`nginxinc/nginx-unprivileged` ships `/etc/nginx/conf.d/default.conf` with a server block
listening on **8080** — the same port ours uses. Since `proxy/nginx.conf` does
`include /etc/nginx/conf.d/*.conf`, both load, and the stock default (a catch-all
`server_name _`) can win, serving the nginx welcome page instead of proxying. Looks like
"my routes did nothing".

**Verified live 2026-07-18:** two `server` blocks on `:8080` both with `server_name _` is
**not** an error — `nginx -t` reports `syntax is ok` / `test is successful` and the first
block loaded silently becomes the default server. **`nginx -t` cannot catch this.** Only
an actual request does.

## Fix / How to avoid

The proxy/Dockerfile does `rm -f /etc/nginx/conf.d/default.conf` before copying ours. Keep that
line. If you ever change the base image or its tag, re-check what it drops into `proxy/conf.d/`
— `docker run --rm --entrypoint ls <image> /etc/nginx/conf.d/`.

## Sources
- pkgcache: `proxy/Dockerfile`

---

# Client fixers that aren't reversible (or that eat third-party repos)

## Problem

The client scripts edit system package-manager config on machines that need to keep
working. Two ways to ruin someone's afternoon: a broad `sed` that also rewrites PPAs /
docker / rpmfusion entries (pointing them at a cache that has no such upstream ⇒ every
`apt update` now fails), or an edit with no backup (no way back when the cache host is
down).

## Fix / How to avoid

Match `scripts/fix-debian` / `scripts/fix-fedora` exactly:

- match on the **distro's own hostnames only** (`*.debian.org`, `security.debian.org`) —
  never a blanket URL rewrite;
- back up every file touched, **once**, as `<file>.pkgcache.orig` (guard with
  `[ -f "$f.pkgcache.orig" ] ||` so a re-run can't overwrite a good backup with an
  already-patched file);
- ship `--revert` and test it;
- re-exec as root via `exec sudo -E "$0" "$@"` so `CACHE=` survives;
- rewrite the **more specific** path first (`security.debian.org/debian-security` before
  the generic `*.debian.org/debian`) or the generic pattern eats it;
- escape `$releasever`/`$basearch` in heredocs (`\$releasever`) — they are for dnf to
  expand, not the shell.

## Sources
- pkgcache: `scripts/fix-debian`, `scripts/fix-fedora`

---

# `dnf config-manager setopt` does not edit the `.repo` file — restoring a backup does not undo it  [OBSERVED 2026-07-25]

## Problem

`containers/fedora/pkgcache-setup` disabled the stock repos with
`dnf config-manager setopt fedora.enabled=0 updates.enabled=0` (copied from
`scripts/fix-fedora`) and implemented `--revert` by restoring a pristine copy of
`/etc/yum.repos.d/fedora.repo`. The restore worked — the file on disk read `enabled=1`
again — and `dnf repolist` still reported `fedora` and `updates` as **disabled**.

dnf5's `setopt` never touches the `.repo` file. It writes
`/etc/dnf/repos.override.d/99-config_manager.repo`, and an override there **outranks** the
`.repo` file. So the two mechanisms are not interchangeable: a file-based revert cannot
undo a `setopt`, and reading the `.repo` file tells you nothing about the effective state.

## Fix / How to avoid

Pick one mechanism per direction and stay in it:

- **Undo with the same tool you did it with.** `setopt fedora.enabled=0` is undone by
  `dnf config-manager unsetopt fedora.enabled` (or `setopt …=1`), not by restoring a file.
  `scripts/fix-fedora` is fine — it uses `setopt` both ways.
- **In an image, edit the file.** `pkgcache-setup` now flips `enabled=` with a
  section-aware `awk` (only inside `[fedora]` / `[updates]` — a blind
  `s/^enabled=1/enabled=0/` would also hit `-debuginfo`/`-source` sections in a file where
  those ship enabled), so backup/restore is symmetric. Bonus: no `dnf5-plugins` dependency,
  so it works on a minimal base image.
- It also clears a stale `99-config_manager.repo` override (best-effort `unsetopt`), since
  one left by an earlier run would silently outrank the file edit.
- **Verify effective state, not file contents** — `dnf repolist` / `repolist --all`, in a
  container, was what exposed this. The script now re-reads the section it just wrote and
  fails loudly instead of trusting the edit.

## Sources
- pkgcache: `containers/fedora/pkgcache-setup` (`set_section_enabled`, `clear_config_manager_override`)
- pkgcache: `scripts/fix-fedora` (`set_repo_enabled` — the `setopt`-both-ways version)

---

# Ubuntu's `s-maxage=3300` silently stretches a 60 s metadata TTL to 55 minutes  [OBSERVED 2026-07-25]

## Problem

`/ubuntu/` was first written as a copy of the `/debian/` block — same 60 s metadata TTL,
same `proxy_cache_revalidate on`, `proxy_ignore_headers` only in the `.deb` sub-location.
That is correct for Debian and wrong for Ubuntu.

Both Ubuntu upstreams send `Cache-Control: max-age=0, proxy-revalidate, s-maxage=3300`.
nginx checks **`s-maxage` before `max-age`**, and upstream `Cache-Control` outranks
`proxy_cache_valid`, so the block that *reads* "60s" actually caches `InRelease` and
`Packages.gz` for **3300 s**. `max-age=0` does not help; `proxy-revalidate` is not
implemented by nginx at all. The result is a 55-minute window in which metadata and the
packages it indexes can disagree — i.e. apt **hash sum mismatch**, the exact failure the
metadata/package TTL split exists to prevent. Nothing in `nginx -t`, the logs, or a
MISS→HIT check reveals it; the config looks right.

## Fix / How to avoid

- The Ubuntu **metadata** locations carry `proxy_ignore_headers Cache-Control Expires`
  alongside `proxy_cache_valid 200 302 60s`. The in-config rule against ignoring
  `Cache-Control` in metadata locations bans *lengthening* a metadata TTL; clamping 3300 s
  **down** to 60 s serves the same goal. The config comment now names the direction, so the
  next reader does not have to re-derive it.
- **Measure every new upstream's `Cache-Control` before writing its route block**:
  `curl -sI <upstream-metadata-url> | grep -i cache-control`. Debian says `max-age=120`
  (fine, defer to it); Ubuntu says `s-maxage=3300` (clamp). Assume nothing from the
  neighbouring block.
- **Verify retention, don't read it.** Two cheap proofs, both used here: `valid_sec - date`
  from the cache-file header (see `distilled.md` for the offsets), and a re-request just
  past the intended TTL — `REVALIDATED` means the entry really did expire, `HIT` means it
  did not.

## Sources
- pkgcache: `proxy/conf.d/pkgcache.conf` (`location /ubuntu/`, `location /ubuntu-security/`)
- pkgcache: `context/distilled.md` § Ubuntu: `s-maxage` beats `max-age`

---

# A verification grep that matches your own backup file  [OBSERVED 2026-07-25]

## Problem

`containers/ubuntu/pkgcache-setup` rewrites apt's sources, then asserts the stock URLs are
gone — originally with a recursive `grep` over `/etc/apt/sources.list` **and**
`/etc/apt/sources.list.d/`. It backs each file up as `<file>.pkgcache-bak` first, and that
backup legitimately still contains the stock URLs.

On 22.04 this passed: the file is `/etc/apt/sources.list`, so the backup lands *beside* it,
outside the directory being walked. On 24.04 (deb822) the file is
`/etc/apt/sources.list.d/ubuntu.sources`, so the backup lands *inside* the walked
directory — the assertion matched its own backup and aborted a build whose rewrite was
perfectly correct. The bug was in the check, not the change it was checking.

## Fix / How to avoid

- **Derive the verification set from the same list you edited.** The script now has one
  `apt_source_files()` producing the exact files apt reads (`sources.list`, `*.list`,
  `*.sources` — never `*.pkgcache-bak`), and both the rewrite and the assertion iterate it.
  A recursive `grep -r` over a config directory is almost always wrong once you write
  backups into it.
- Corollary for any backup-before-edit script: **know whether your backup lands inside the
  tree you scan.** It depends on the distro release, which is exactly the sort of thing that
  passes on the version you tested and fails on the next.
- The assertion still earned its place — it failed loudly at build time instead of shipping
  a half-configured image. Keep such checks; just scope them correctly.

## Sources
- pkgcache: `containers/ubuntu/pkgcache-setup` (`apt_source_files`, `grep_apt_sources`)
