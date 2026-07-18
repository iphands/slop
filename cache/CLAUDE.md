# pkgcache — LAN Package Caching Proxy (Debian + Fedora)

A single **nginx reverse caching proxy**, containerized, that caches OS package
downloads for a homelab fleet of **Debian 13 (Trixie)** and **Fedora 44** machines.
First machine to download a package pulls it from the internet; every machine after
that is served from the LAN cache.

> **Sibling projects:** [`../qbots`](../qbots/AGENTS.md) and [`../qctrl`](../qctrl/AGENTS.md)
> are Rust/Q2 projects under the same `slop` umbrella. This one is **not Rust** — it is
> config + shell. The *workflow discipline* (plans, context/, honesty, commit rules) is
> shared; the toolchain is not. Don't reach for `cargo` here.

---

## The One Thing That Makes This Hard

**apt and dnf are not the same problem.**

- **Debian** fetches over plain **HTTP** from `deb.debian.org`. Almost anything caches it —
  a forward proxy, lancache, Squid, whatever.
- **Fedora** fetches over **HTTPS + metalink** (dnf asks a metalink service for a mirror
  list, then TLS-fetches from a *dynamically chosen* mirror). A **forward proxy sees only
  an encrypted `CONNECT` tunnel** and cannot cache a single byte of it. And the mirror
  varies run to run, so even URL-keyed caching would miss.

That single fact is why this is a **reverse** cache, not a proxy:

> The client speaks **plain HTTP to nginx**; **nginx originates the upstream TLS itself**
> and stores the `.rpm`. Clients are pointed at fixed `baseurl`s (no metalink), so the
> cache key is stable.

**Why plain HTTP client→cache is safe:** packages stay **GPG-signed end-to-end**. apt/dnf
verify signatures after download; the transport is not the trust boundary. `gpgcheck=1`
and `Signed-By:` **must stay on** — they are what makes the HTTP hop acceptable. Never
"simplify" by disabling them.

The tradeoff we accept: this is **not transparent**. Clients must be reconfigured to point
at the cache. `scripts/fix-debian` and `scripts/fix-fedora` automate that (reversibly).

---

## Project Goal

One small container, one bind-mounted cache dir, rootless, that a handful of Debian and
Fedora boxes point at — with client-side setup reduced to running one script.

### Core Properties
- **Correct before fast.** Serving stale repo metadata breaks apt/dnf with hash
  mismatches. The metadata/package TTL split (below) is the heart of this project.
- **Rootless.** Runs as arbitrary `APP_UID:APP_GID` (default `1000:1000`); every writable
  path is under the mounted volume or `/tmp`.
- **Engine-agnostic.** `build`/`publish`/`run` auto-detect and **prefer podman**, fall
  back to docker; `RUNTIME=` forces one.
- **Reversible client changes.** Every client fixer backs up what it edits and has
  `--revert`.

---

## Critical Facts

### 1. The TTL split is correctness-critical — do not "unify" it

| Content | TTL | Why |
|---|---|---|
| **Package files** (`*.deb`, `*.udeb`, `*.rpm`) | **365d** | Immutable. A given filename's bytes never change; a new version is a new filename. |
| **Repo metadata** (`Release`, `InRelease`, `Packages*`, `repomd.xml`, `*.xml`, `by-hash/…`) | **60s** + `proxy_cache_revalidate on` | Regenerated constantly upstream. Metadata that is newer/older than the packages it indexes ⇒ **apt/dnf hash-mismatch failures**. Revalidation makes the 60s refresh a cheap `If-Modified-Since`, not a re-download. |

If someone reports "apt says hash sum mismatch" or "dnf says repomd.xml doesn't match",
**suspect metadata TTL / cache-key bugs first**, not upstream.

### 2. A nested regex `location` cannot inherit the parent's `proxy_pass` path remap

This is the sharpest edge in `conf.d/pkgcache.conf`. In nginx:

- A **prefix** `location /fedora/ { proxy_pass https://host/pub/fedora/; }` replaces the
  matched prefix with the `proxy_pass` URI path. Good.
- A **nested regex** `location ~* \.rpm$` **may not have a URI part on `proxy_pass` at
  all** — nginx rejects it at config-parse time (`proxy_pass cannot have URI part in
  location given by regular expression…`). Drop the path to satisfy it and the remap is
  **silently lost**, so it must be re-applied by hand:
  ```nginx
  rewrite ^/fedora/(.*)$ /pub/fedora/$1 break;
  proxy_pass https://dl.fedoraproject.org;   # no URI path!
  ```
  The Debian blocks exploit the same rule in the *opposite* direction: upstream path
  `/debian/…` already matches the client path, so `proxy_pass http://deb.debian.org;`
  (bare host, no path) is correct there.

**Any edit to the regex sub-locations must be verified with a real fetch**, not by
reading. See Verification below.

### 3. Port 8080 vs 3129

- The container **always** listens on **8080** internally (unprivileged nginx base).
- `./run` publishes host `PORT` (default `8080`) → container `8080`.
- The client fixer scripts default to `CACHE=http://noir.lan:3129` — the **live homelab
  deployment**, which is published on **3129**.

These two defaults intentionally disagree. When changing one, check the other, and prefer
`PORT=3129 ./run` on the real host over editing the scripts' default.

### 4. Fedora upstream is the master mirror

`/fedora/` points at `dl.fedoraproject.org` (Fedora's **master**). Fine for a handful of
machines; **rude at scale**. If throughput matters, repoint at a regional mirror in
`conf.d/pkgcache.conf` — keep the `/pub/fedora/` path shape or fix both the prefix
`proxy_pass` and the `.rpm` `rewrite` together.

### 5. Rootless podman uid mapping

Host `APP_UID` is **not** the in-container uid under rootless podman unless you pass
`EXTRA_ARGS="--userns=keep-id:uid=1000,gid=1000"` and own `CACHE_DIR` as your host user.
docker and rootful podman need no flag. Permission-denied on the cache tree at startup is
almost always this.

### 6. Container engine: **docker here, podman in production**

The two environments are opposites, and both matter:

| | Engine | Notes |
|---|---|---|
| **Dev machine** (this one) | **docker** | Rootless podman is broken here — pulls fail with `potentially insufficient UIDs or GIDs available in user namespace`, wanting `podman system migrate`. |
| **Live host** `noir.lan` | **podman** | No docker installed at all. This is what actually runs the cache. |

**Verify with `RUNTIME=docker`.** Do *not* "fix" `build`/`run`/`publish` to prefer docker —
their podman-preferred auto-detection is exactly what makes them work unchanged on the live
host, and `RUNTIME` already overrides it per-invocation.

The cost of this asymmetry: **`--userns=keep-id` cannot be exercised here** (it only applies
to rootless podman). Anything depending on it is unverified until a live deploy — say so
rather than implying it was tested.

---

## Architecture

### Stack
- **nginx** `proxy_cache` on `nginxinc/nginx-unprivileged:1.27-alpine`. No app code.
- **Shell** (`bash`, `set -euo pipefail`) for build/publish/run + client fixers.
- **podman** (preferred) / docker.

### Layout
```text
cache/
├── CLAUDE.md            # This file
├── README.md            # User-facing docs — KEEP IN SYNC with behavior changes
├── Dockerfile           # nginx-unprivileged + our config; HEALTHCHECK via busybox wget
├── nginx.conf           # main: temp paths, proxy_cache_path zone, resolver, log_format
├── conf.d/
│   └── pkgcache.conf    # server block: the routes + TTL split  <-- the real logic
├── build                # -> IMAGE:latest + IMAGE:<git-sha>
├── publish              # push to Docker Hub (human runs this)
├── run                  # detached container, bind-mounted cache, --user UID:GID
├── scripts/
│   ├── fix-debian       # rewrite apt sources -> cache (+ --revert)
│   └── fix-fedora       # write baseurl repos, disable metalink (+ --revert)
├── context/             # knowledge base — READ BEFORE NEW WORK
│   ├── plans/
│   │   ├── RULES.md     # plan format + Rules A–D (authoritative; read in full)
│   │   ├── SERIES.md    # plan dependency chain + next free plan number
│   │   ├── NN_example.md # canonical plan template — copy this
│   │   └── completed/   # done plans (git mv here at 100%)
│   ├── distilled.md     # confirmed nginx/apt/dnf facts + open questions
│   ├── pitfalls.md      # bugs & gotchas, especially multi-attempt fixes
│   └── high_level.md    # why nginx over the alternatives; revisit-if triggers
└── vendor/              # READ-ONLY upstream source/docs (currently empty)
```

### Route table

| Client path | Upstream | Transport |
|---|---|---|
| `/debian/…` | `http://deb.debian.org/debian/…` | HTTP (Fastly CDN) |
| `/debian-security/…` | `http://security.debian.org/debian-security/…` | HTTP |
| `/fedora/…` | `https://dl.fedoraproject.org/pub/fedora/…` | **nginx originates TLS** |
| `/healthz` | — | `200 ok` (used by HEALTHCHECK) |
| `/` | — | route banner |

### Load-bearing nginx details
- `proxy_cache_path … levels=1:2 keys_zone=pkg:64m inactive=365d max_size=100g use_temp_path=off`
  — `use_temp_path=off` avoids a cross-device rename into the bind mount.
- All `*_temp_path` directives point under `/var/cache/nginx` (the volume), one level deep,
  because nginx's mkdir is single-level on a fresh mount.
- `resolver 127.0.0.11 1.1.1.1` — upstreams are proxied **by hostname**, so a resolver is
  mandatory. `127.0.0.11` is the docker/podman embedded DNS; `1.1.1.1` covers host/bridge
  networks where it's absent.
- `add_header X-Cache-Status $upstream_cache_status always;` — the verification handle.
- `proxy_cache_lock on` (thundering-herd on a cold `.rpm`) and `proxy_cache_use_stale
  updating error timeout http_5xx` (survive brief upstream outages).
- Stock `default.conf` is **deleted** in the Dockerfile — it also binds `:8080` and would
  shadow ours.

---

## Development Workflow

### 1. Planning — read `context/plans/RULES.md` first
**`context/plans/RULES.md` is authoritative** (plan/tracker format, Rules A–D). Read it in
full before writing a plan. `context/plans/SERIES.md` holds the dependency chain and the
next free plan number; `context/plans/NN_example.md` is the template to copy.

Scaled to this project's size — RULES.md has the full table:
- Small/obvious change (a TTL, a comment, a flag)? Just do it, verify (Rule A), commit.
- Anything structural (new upstream, new route shape, cache-key change, a new client
  fixer, a second service, TLS, auth)? Plan first, paired tracker, `git mv` both to
  `completed/` the moment it's 100% done (Rule C).

### 2. Knowledge Management (`context/`) — read before new work
- **`context/distilled.md`** — confirmed nginx caching semantics, apt deb822 format, dnf
  metalink behavior, repo layout facts. Entries are tagged `[SOURCE]`/`[REPO]`/`[LIVE]`;
  **don't upgrade a tag without doing the work.** It also carries **Open Questions** —
  check them before trusting a related assumption.
- **`context/pitfalls.md`** — every bug/gotcha, **especially multi-attempt fixes**.
  Template: `# Title → Problem → Fix / How to avoid → Sources`. Mark real incidents
  `[OBSERVED YYYY-MM-DD]` to distinguish them from seeded hazards. Cross-cutting ones also
  go up to `../context/pitfalls.md` per the slop convention; nginx/apt/dnf specifics stay
  local.
- **`context/high_level.md`** — why nginx over lancache/apt-cacher-ng/Squid/Nexus, with
  explicit *revisit-if* triggers. The user-facing table lives in `README.md`; the deeper
  rationale lives here.
- **Rule D:** a plan isn't done when the config works — it's done when what you learned is
  on disk in these files.

### 3. Testing — there is no unit test suite; **you must exercise it**
Config-only projects fail *at runtime*, so "it looks right" is never done.

```bash
# RUNTIME=docker: this machine has docker; noir.lan has podman. See Critical Fact #6.
export RUNTIME=docker
./build && PORT=8080 CACHE_DIR=/tmp/pkgcache-test ./run

# nginx accepted the config at all:
docker logs pkgcache

# health + routes:
curl -f http://localhost:8080/healthz            # -> ok

# metadata path (short TTL, revalidated):
curl -sI http://localhost:8080/debian/dists/trixie/InRelease | grep -i x-cache-status

# package path (the regex sub-location — the part that breaks):
curl -sI http://localhost:8080/debian/pool/main/c/cowsay/<file>.deb | grep -i x-cache-status

# Fedora, both halves (metadata AND the rewrite-dependent .rpm path):
curl -sI http://localhost:8080/fedora/linux/releases/44/Everything/x86_64/os/repodata/repomd.xml
curl -sI http://localhost:8080/fedora/linux/releases/44/Everything/x86_64/os/Packages/c/<file>.rpm
```
**MISS then HIT on a second identical request** is the pass condition. A `404` on an
`.rpm` that exists upstream ⇒ the `rewrite`/`proxy_pass` remap is wrong (Critical Fact #2).
Also lint the shell: `shellcheck build publish run scripts/*`.

### 4. Never commit broken config
An nginx config typo means the container **crash-loops** — worse than a compile error,
because `./build` still succeeds. Always `./run` and check `logs` before committing.

### 5. Commits

**COMMIT AT EVERY TASK COMPLETION. DO NOT WAIT.**

- Small, frequent, one task per commit. Format: `task(TN): <description>` when working a
  plan; otherwise a plain scoped message (`cache: <description>`).
- **Verify (§3) before every commit.** Do not claim "done" on unverified config.
- Never batch unrelated changes.
- **Never push** — the human pushes after review. No co-author trailers unless asked.
  *(Global rule, `~/.claude/CLAUDE.md`.)*

### 6. Tooling
- **No `tmp/` scripts.** Helpers become real, documented, reusable scripts at the repo
  root or in `scripts/`, with a usage comment block matching the existing style
  (`set -euo pipefail`, env-var knobs with `${VAR:-default}`, `--revert` where destructive).
- New client fixers (`scripts/fix-<distro>`) should match `fix-debian`/`fix-fedora`:
  root re-exec via `sudo -E`, idempotent, backup-before-edit, `--revert`, ending with a
  `curl … | grep X-Cache-Status` verification hint.

---

## Constraints & Rules

1. **Never weaken package verification.** `gpgcheck=1`, `Signed-By:`, `skip_if_unavailable=False`
   stay. HTTP client→cache is only safe *because* signatures are checked.
2. **Never lengthen metadata TTL** to "improve hit rate." That trades correctness for a
   metric nobody asked for.
3. **Stay rootless.** No `USER root` at runtime, no privileged ports, no writable path
   outside the volume or `/tmp`.
4. **Client changes must be reversible.** Backup-before-edit + `--revert`, always.
   Never touch third-party repos (PPAs, docker, rpmfusion) — only the distro's own.
5. **Env-var knobs, no hardcoding.** Every path/port/uid/image is `${VAR:-default}`.
6. **Never commit cache contents or build artifacts.** `/srv/pkgcache` and any local test
   cache dir stay out of git. `vendor/` is cloned, not authored — gitignored.
7. **README.md is user-facing and must stay true.** Behavior change ⇒ README edit in the
   same commit.
8. **Honesty.** Do the thing, verify it, then say "done." Never claim something is recorded
   in `distilled.md`/`pitfalls.md`/`CLAUDE.md` unless the bytes are on disk. Never report a
   cache HIT you didn't actually observe. Be direct.

---

## Likely Next Work

Not a committed roadmap — the plausible directions, so a plan can start from context:

1. **More distros.** openSUSE (`zypper`), Arch (`pacman`, `Server =` lines), Alpine, EPEL,
   rpmfusion, Ubuntu. Each is a route block + a `scripts/fix-<distro>`.
2. **Cache observability.** A `/stats` endpoint or a log-scraper reporting HIT ratio and
   on-disk size per upstream.
3. **Prefetch/warm.** Pull the metadata (and hot packages) on a timer so the first client
   of the day isn't the one that pays.
4. **Regional Fedora mirror** selection instead of hammering `dl.fedoraproject.org`.
5. **HTTPS on the cache itself** — only worth it if the LAN stops being trusted; note it
   buys little given GPG already covers integrity.
