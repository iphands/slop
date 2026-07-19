# pkgcache — package caching proxy for Debian & Fedora

A single, lightweight **nginx reverse caching proxy** that caches OS package
downloads for a fleet of **Debian 13 (Trixie)** and **Fedora 44** machines.
First machine to download a package pulls it from the internet; every machine
after that gets it from the local cache over the LAN.

- Cache lives on a **bind-mounted volume** (`/srv/pkgcache/data` by default).
- Runs **rootless** as `APP_UID:APP_GID` (default `1000:1000`).
- `build` / `publish` / `run` **auto-detect podman/docker and prefer podman**.
- An optional second container serves a **stats dashboard** — per-client hit ratios,
  bytes saved, top packages. See [Stats dashboard](#stats-dashboard).

## Why nginx (and not the alternatives)

The hard requirement is caching **both** apt and dnf. Fedora's `dnf` fetches
over **HTTPS + metalink** (dynamic mirror selection), which is what rules most
tools out: a *forward proxy* only sees an encrypted `CONNECT` tunnel and can't
cache what's inside it. nginx is a **reverse cache** — the client speaks HTTP to
nginx, and nginx originates the upstream TLS itself and stores the `.rpm`.
Packages stay GPG-verified end-to-end, so plain HTTP from client→cache is safe.

| Option | Debian 13 | Fedora 44 | Notes |
|---|:---:|:---:|---|
| **nginx `proxy_cache`** ✅ chosen | ✅ | ✅ | Reverse cache, originates upstream TLS. It's nginx — zero staleness risk. Con: you point client repo config at it (this repo does that for you). |
| lancache | ✅ transparent | ❌ | DNS-spoof + HTTP-only cache. Great for Debian (Trixie sources are HTTP), but **cannot** cache Fedora's HTTPS+metalink. |
| apt-cacher-ng | ✅ | ✅ | Purpose-built, has `Remap-fedora`. Works, but perceived as stale/unmaintained. |
| soulteary/apt-proxy | ✅ | ❌ | Actively maintained Go forward-proxy, but **no Fedora handler** (only `centos.go`, hardcoded to `/centos/` + `mirror.centos.org`). |
| Squid | ✅ | ⚠️ | Generic proxy; caching Fedora's HTTPS needs SSL-bump (MITM) which fights the GPG trust model. Fiddly `refresh_pattern` tuning. |
| Nexus / Pulp | ✅ | ✅ | Full artifact managers — heavy (Java/Python), overkill for a homelab. |

## Architecture

Each client path prefix maps to a real upstream mirror:

| Client path | Upstream | |
|---|---|---|
| `/debian/…` | `http://deb.debian.org/debian/…` | Fastly CDN, HTTP, GPG-signed |
| `/debian-security/…` | `http://security.debian.org/debian-security/…` | |
| `/fedora/…` | `https://dl.fedoraproject.org/pub/fedora/…` | nginx originates the HTTPS |

**Caching split (correctness-critical):** package files (`*.deb`, `*.udeb`,
`*.rpm`) are immutable → cached **365 days**. Repo metadata (`Release`,
`InRelease`, `Packages`, `repomd.xml`, `*.xml`, `by-hash/…`) changes constantly
→ cached **60 s** with `proxy_cache_revalidate` (cheap `If-Modified-Since`).
Serving stale metadata would cause apt/dnf hash-mismatch errors, so the split is
deliberate — see `proxy/conf.d/pkgcache.conf`.

> `dl.fedoraproject.org` is Fedora's master mirror — fine for a handful of
> machines. For heavier use, point `/fedora/` at a regional mirror by editing
> `proxy/conf.d/pkgcache.conf`.

## Build / publish / run

Each script takes an optional target — `all` (default), `proxy`, or `stats`:

```bash
./build          # -> iphands/pkgcache{,-stats}:latest + :<git-sha>
./build proxy    # just the caching proxy
./publish        # push to Docker Hub (needs `docker login` / `podman login docker.io`)
./run            # launch both; proxy on :8080, stats on :8081
./run proxy      # just the proxy
```

Common overrides (all env-based):

```bash
RUNTIME=docker ./build                   # force docker (default: prefer podman, then docker)
PORT=3142 ./run                          # different host port for the proxy
STATS_PORT=3143 ./run                    # different host port for the dashboard
CACHE_DIR=/mnt/tank/pkgcache/data ./run  # different cache location
APP_UID=1500 APP_GID=1500 ./run          # different user
IMAGE=you/pkgcache ./build ./publish     # your own image name
```

### Directory layout

`./run` provisions three directories, one purpose each. `LOGS_DIR` and `STATS_DIR`
default to siblings of `CACHE_DIR`:

| Directory | proxy | stats | Contents |
|---|---|---|---|
| `CACHE_DIR` (`…/data`) | `:/var/cache/nginx` rw | `:/cache` **ro** | nginx's package cache |
| `LOGS_DIR` (`…/logs`) | `:/logs` rw | `:/logs` rw | the machine-readable access log |
| `STATS_DIR` (`…/frontend`) | — | `:/data` rw | `stats.sqlite` and friends |

Both containers **must** run as the same uid — nginx writes the log, the stats service
reads it. `./run` handles that; if you launch by hand, keep them identical.

All three scripts auto-detect the container engine and **prefer podman**; set
`RUNTIME=docker` (or `RUNTIME=podman`) to force one.

Check it's alive:

```bash
curl -f http://<cache-host>:8080/healthz          # -> ok
curl -sI http://<cache-host>:8080/debian/dists/trixie/InRelease | grep X-Cache-Status
```

> **Rootless podman:** the host `APP_UID` is not the in-container uid unless you
> add `EXTRA_ARGS="--userns=keep-id:uid=1000,gid=1000" ./run` and own `CACHE_DIR`
> as your host user. docker and rootful podman need no extra flag.

## Client configuration

**Easiest:** copy `scripts/fix-*` to the client and run the fixer for its distro
(defaults to `http://noir.lan:3129`; override with `CACHE=`). Note `scripts/noir/` is the
*host* deployment recipe and has no business on a client:

```bash
sudo ./scripts/fix-debian      # Debian 13: rewrite apt sources -> cache, apt update
sudo ./scripts/fix-fedora      # Fedora 44: add cache repos, disable metalink, makecache
# undo either with:  sudo ./scripts/fix-debian --revert   /   fix-fedora --revert
# different endpoint:  CACHE=http://noir.lan:3129 sudo -E ./scripts/fix-debian
```

`fix-debian` backs up every file it edits as `<file>.pkgcache.orig` and leaves
third-party repos (PPAs, docker, …) untouched. `fix-fedora` writes
`/etc/yum.repos.d/fedora-cache.repo` and disables the stock metalink repos.

The manual equivalents are below. Replace `CACHE:PORT` with your cache
host/port. Packages stay GPG-verified, so HTTP to the cache is safe.

### Debian 13 (Trixie) — `/etc/apt/sources.list.d/debian.sources`

```
Types: deb
URIs: http://CACHE:PORT/debian
Suites: trixie trixie-updates
Components: main contrib non-free non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg

Types: deb
URIs: http://CACHE:PORT/debian-security
Suites: trixie-security
Components: main contrib non-free non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
```

Then `sudo apt update`.

### Fedora 44 — `/etc/yum.repos.d/fedora-cache.repo`

```ini
[fedora-cache]
name=Fedora $releasever - $basearch (cached)
baseurl=http://CACHE:PORT/fedora/linux/releases/$releasever/Everything/$basearch/os/
enabled=1
gpgcheck=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch

[updates-cache]
name=Fedora $releasever - $basearch - Updates (cached)
baseurl=http://CACHE:PORT/fedora/linux/updates/$releasever/Everything/$basearch/
enabled=1
gpgcheck=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch
```

Disable the stock metalink repos so dnf uses the cache:

```bash
sudo dnf config-manager setopt fedora.enabled=0 updates.enabled=0
sudo dnf makecache
```

## Verifying the cache works

On two different clients, install the same package and watch the second one
hit the cache:

```bash
# client A
sudo apt install -d cowsay        # or: sudo dnf install cowsay
# client B (same command) -> served from cache; on the cache host:
curl -sI http://CACHE:PORT/debian/pool/main/c/cowsay/<file>.deb | grep X-Cache-Status
# X-Cache-Status: HIT
```

The cache directory (`/srv/pkgcache/data`) grows as packages are cached.

## Stats dashboard

![The pkgcache stats dashboard](docs/dashboard.jpg)

A second, optional container reads the proxy's access log off the shared volume and serves
a dashboard on **:8081**. It is coupled to the proxy **only through the filesystem** — no
shared network — so it can never affect package serving, and if it is down for a week it
simply catches up.

```bash
./build stats && ./run stats
curl -f http://<cache-host>:8081/healthz
```

It shows, split between **packages** and **repo metadata** (which are very different
populations — metadata has a 60 s TTL and is *supposed* to miss):

- global hit ratio **by bytes** and by requests, plus lifetime bytes saved
- a per-client table: hit ratio, bytes served, bytes saved, 24 h sparkline
- traffic and hit-ratio over 24 h / 7 d / 30 d
- top packages, and a per-client drilldown of what each machine pulled
- cache fullness against `max_size` — worth watching, since eviction quietly turns
  HITs back into MISSes

The proxy writes `LOGS_DIR/access-YYYY-MM-DD.log` whether or not the stats container is
running; the stats service prunes files once they are fully ingested and older than its
retention window (3 days by default).
