# pkgcache — package caching proxy for Debian & Fedora

A single, lightweight **nginx reverse caching proxy** that caches OS package
downloads for a fleet of **Debian 13 (Trixie)** and **Fedora 44** machines.
First machine to download a package pulls it from the internet; every machine
after that gets it from the local cache over the LAN.

- Cache lives on a **bind-mounted volume** (`/srv/pkgcache` by default).
- Runs **rootless** as `APP_UID:APP_GID` (default `1000:1000`).
- `build` / `publish` / `run` **auto-detect podman/docker and prefer podman**.

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
deliberate — see `conf.d/pkgcache.conf`.

> `dl.fedoraproject.org` is Fedora's master mirror — fine for a handful of
> machines. For heavier use, point `/fedora/` at a regional mirror by editing
> `conf.d/pkgcache.conf`.

## Build / publish / run

```bash
./build          # -> iphands/pkgcache:latest + iphands/pkgcache:<git-sha>
./publish        # push to Docker Hub (needs `docker login` / `podman login docker.io`)
./run            # launch on :8080, cache in /srv/pkgcache, as uid 1000:1000
```

Common overrides (all env-based):

```bash
RUNTIME=docker ./build                 # force docker (default: prefer podman, then docker)
PORT=3142 ./run                        # different host port
CACHE_DIR=/mnt/tank/pkgcache ./run     # different cache location
APP_UID=1500 APP_GID=1500 ./run        # different user
IMAGE=you/pkgcache ./build ./publish   # your own image name
```

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
(`scripts/noir/` is the *host* deployment recipe — it has no business on a client)
(defaults to `http://noir.lan:3129`; override with `CACHE=`):

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

The cache directory (`/srv/pkgcache`) grows as packages are cached.
