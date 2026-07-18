# High-Level — why these tools, and what we rejected

Short rationale for the dependency/architecture choices. Keep entries brief; deep
technical detail belongs in `distilled.md`, failure modes in `pitfalls.md`.

---

## The deciding constraint

Cache **both** apt and dnf, for a homelab fleet, in one small thing.

Debian is easy (plain HTTP). **Fedora is the whole problem:** dnf uses
**HTTPS + metalink** — it asks a metalink service for a mirror list, then TLS-fetches from
a mirror chosen at runtime. So:

- a **forward proxy** sees only an encrypted `CONNECT` tunnel → can cache nothing;
- the mirror **varies per run** → even URL-keyed caching would miss.

Only a **reverse cache** works: the client speaks plain HTTP to us, and *we* originate the
upstream TLS. That single constraint eliminates most of the field below.

## Caching layer

| Option | Debian | Fedora | Verdict |
|---|:---:|:---:|---|
| **nginx `proxy_cache`** | ✅ | ✅ | **Chosen.** Reverse cache, originates upstream TLS itself. Boringly well-understood, tiny, no runtime beyond nginx. Con: not transparent — clients must be pointed at it (automated by `scripts/`). |
| lancache | ✅ transparent | ❌ | DNS-spoof + HTTP-only. Lovely for Debian (Trixie is HTTP), **cannot** touch Fedora's HTTPS+metalink. |
| apt-cacher-ng | ✅ | ✅ | Purpose-built, has `Remap-fedora`. Genuinely works; rejected on maintenance/staleness perception, not capability. The closest runner-up. |
| soulteary/apt-proxy | ✅ | ❌ | Maintained Go forward-proxy, but **no Fedora handler** — only `centos.go`, hardcoded to `/centos/` + `mirror.centos.org`. |
| Squid | ✅ | ⚠️ | Generic forward proxy; caching Fedora's HTTPS needs **SSL-bump (MITM)**, which fights the GPG trust model for no gain. Fiddly `refresh_pattern` tuning. |
| Nexus / Pulp | ✅ | ✅ | Full artifact managers. Correct and capable, and *wildly* oversized (JVM/Python stacks) for a handful of LAN boxes. |

**Revisit if:** the fleet grows past homelab scale, or we need auth/retention policy/
multi-repo governance — that's when Pulp or Nexus stops being overkill.

## Base image — `nginxinc/nginx-unprivileged:1.27-alpine`

Vs. stock `nginx`: runs as an arbitrary non-root `--user`, listens on 8080, and keeps all
writable state relocatable — which is the whole reason `./run` can hand it any
`APP_UID:APP_GID`. Alpine for size. Cost: busybox `wget` instead of `curl` (HEALTHCHECK
uses `wget`), and it ships a `default.conf` that must be deleted (see `pitfalls.md`).

## Container engine — podman preferred, docker supported

`build`/`publish`/`run` auto-detect and prefer **podman** (rootless by default, daemonless,
the host's actual setup), falling back to docker. `RUNTIME=` forces either. The cost is
the rootless uid-mapping wrinkle documented in `pitfalls.md`; the benefit is not running a
root daemon to serve `.deb` files.

## Implementation language — none (config + shell)

Deliberately no application code. This is nginx config plus `bash` glue; every "feature"
should first be attempted as a config change. A Go/Rust service would buy custom stats and
prefetch logic (SERIES Plans 02/05) at the cost of a build, a test suite, a dependency
tree, and a thing to keep patched.

**Revisit if:** Plans 02 (observability) and 05 (prefetch) both land and log-parsing shell
becomes the ugly part. Until then, `log_format cache` + `awk` beats a daemon.

## Transport: plain HTTP client → cache

Not laziness — **GPG signature verification is end-to-end** in both apt (`Signed-By:`) and
dnf (`gpgcheck=1`). The transport is not the trust boundary; a MITM on the LAN can deny
service but cannot forge a package. TLS on the cache itself (SERIES Plan 06) adds
confidentiality of *which* packages are being installed, which is not the homelab threat
model. Recorded so the question has a written answer.

**This is also why the GPG settings are non-negotiable** — they are the load-bearing part
of the design, not a default nobody touched.
