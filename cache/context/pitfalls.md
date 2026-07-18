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
> **Two entries are real observations, not seeds** — both found 2026-07-18 while probing
> the Plan 02 stats log format against a live nginx: *"A variable in the `access_log` path
> silently logs NOTHING without a valid `root`"* and *"Logging `$uri` mis-files every
> rewritten request"*. Both would have shipped.

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
