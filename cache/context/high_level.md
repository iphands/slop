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

## Implementation language — config + shell for the proxy, **Rust for the stats service**

**The revisit-if trigger fired (2026-07-18).** This section previously argued for no
application code at all, with the caveat *"revisit if observability and prefetch land and
log-parsing shell becomes the ugly part."* Observability (SERIES Plans 02–05) is that
work, and the shell version does not survive contact with the requirements. Recording the
reasoning honestly rather than quietly deleting the old position:

**What the shell alternative would actually have been.** `awk` over a growing log, run
from cron. It falls over on every requirement that matters:

| Requirement | Shell |
|---|---|
| Don't double-count across runs | needs byte-offset checkpointing — no natural way to do it atomically with the aggregate write |
| Survive a crash mid-run | no transaction; a partial write silently corrupts totals forever |
| Per-client × per-repo × per-hour aggregation | nested awk arrays, then re-derived on every run |
| 30-day retention + rollups | re-scan everything, every time |
| Serve a dashboard | a second thing entirely |
| Be *correct* | unverifiable — nothing to test |

The decisive one is the first two: the whole design rests on *aggregate increments and the
checkpoint offset committing in the same transaction*, and shell has no way to express
that. Everything else is inconvenience; that one is a correctness ceiling.

**What Rust costs, honestly:** a build, a toolchain, a dependency tree, and something to
keep patched — exactly what the old text warned about. Mitigations taken: the ingest core
is a dependency-free crate (`serde` + `thiserror` only) so the logic that matters is
testable in under a second; the binary is static musl in a ~15 MB image with no runtime
deps; and the proxy half is untouched, so a stats outage cannot affect package serving.

**What Rust buys beyond feasibility:** the first real test suite in this repo. `RULES.md`
Rule A exists because nginx config has no compiler; `stats/` does, and the parsing,
classification and aggregation are pure functions with real unit tests.

**Boundary to hold:** the proxy stays config-only. Every proxy "feature" is attempted as
an nginx directive first. Rust lives in `stats/` and does not leak back.

## Database — SQLite (`rusqlite`, bundled)

**The first database anywhere in the slop family** — neither qbots nor qctrl has one, not
even transitively. There was no house precedent to inherit, so the choice was made on
boring-and-obvious grounds.

| Option | Verdict |
|---|---|
| **SQLite** ✅ chosen | Single file, no server, transactional (which the correctness invariant requires), and `rusqlite`'s `bundled` feature vendors the amalgamation so the runtime image needs no `libsqlite3` and the version is pinned to the source tree rather than the base image. |
| Plain files (JSON/CBOR) | What qctrl does for `favorites.json` / `rotation.yaml`. Fine at that scale; here it cannot give an atomic aggregate-plus-offset write, which is the whole design. |
| `sled` / `redb` | Embedded KV, no SQL. Would mean hand-rolling every `GROUP BY` the dashboard needs. |
| Postgres / MySQL | A server to run, back up and patch, for a homelab dataset measured in megabytes. |

Cost: `musl-dev` in the build stage and ~30 s of C compile, cached by a BuildKit mount.

**Revisit if:** the dataset stops fitting the "aggregate on ingest, tiny hourly buckets"
model — e.g. if per-request retention is ever wanted. It isn't today; 30 days of hourly
buckets is under a megabyte in practice.

## Transport: plain HTTP client → cache

Not laziness — **GPG signature verification is end-to-end** in both apt (`Signed-By:`) and
dnf (`gpgcheck=1`). The transport is not the trust boundary; a MITM on the LAN can deny
service but cannot forge a package. TLS on the cache itself (SERIES Plan 06) adds
confidentiality of *which* packages are being installed, which is not the homelab threat
model. Recorded so the question has a written answer.

**This is also why the GPG settings are non-negotiable** — they are the load-bearing part
of the design, not a default nobody touched.
