# Plan 06_3 — Ubuntu 22.04 (jammy) route + client container

> **Status**: in-progress
> **Created**: 2026-07-25
> **Depends on**: Plan 01
> **Goal**: Cache Ubuntu 22.04 (jammy) packages through the proxy, and ship a
> `containers/ubuntu` image whose apt is already pointed at the cache.
> **Agent**: main session (interactive)

---

> **Before writing any config, re-read `context/plans/RULES.md` in full** — especially
> Rule A (prove it runs and caches; there is no compiler here).
> For historical context, completed plans live in `context/plans/completed/`.

---

## TL;DR

**What**: Add `/ubuntu/` and `/ubuntu-security/` route blocks, then an `ubuntu:22.04`
container image with apt pre-pointed at the cache — the Ubuntu counterpart of
`containers/fedora` (shipped 2026-07-25, commit `ab2da3ccc`).

**Deliverables**:
1. `/ubuntu/` → `http://archive.ubuntu.com/ubuntu/` with the metadata/package TTL split
2. `/ubuntu-security/` → `http://security.ubuntu.com/ubuntu/` (path remap ⇒ `rewrite` in
   the `.deb` sub-location)
3. `containers/ubuntu/{Dockerfile,pkgcache-setup,build,publish}`
4. README + CLAUDE.md route table + `distilled.md` / `pitfalls.md` harvest (Rule D)

**Estimated effort**: Small–Medium (half day)

**Explicitly out of scope**: `scripts/fix-ubuntu` (the *host* client fixer). Not
requested; the container is the deliverable. It is a natural follow-up and is listed as P2
in Critical Files so the omission is visible rather than forgotten.

---

## Context

### Why a plan for this at all

`RULES.md` § *When Does a Change Need a Plan?* — "A new upstream/route block" ⇒ **Yes**.
The request was "an Ubuntu container", but an Ubuntu container is useless without an
Ubuntu route: the proxy today knows only `/debian/`, `/debian-security/`, `/fedora/`, so
every apt fetch from such a container would 404.

### Key Facts

All fetched 2026-07-25, not guessed.

**Client shape.** `ubuntu:22.04` is codename **jammy** and still uses the *classic*
one-line `/etc/apt/sources.list` — **not** deb822. `/etc/apt/sources.list.d/` is empty.
(Ubuntu moved to `ubuntu.sources` deb822 in 24.04, so `fix-debian`'s deb822 assumption
does **not** transfer.) The stock file is 10 `deb` lines: 7 × `archive.ubuntu.com/ubuntu/`
(jammy, jammy-updates, jammy-backports across main/restricted/universe/multiverse) and
3 × `security.ubuntu.com/ubuntu/` (jammy-security).

**Two upstreams, and only one of them is path-identical.**

| Client path | Upstream | Path remap? |
|---|---|---|
| `/ubuntu/…` | `http://archive.ubuntu.com/ubuntu/…` | **No** — prefix already matches |
| `/ubuntu-security/…` | `http://security.ubuntu.com/ubuntu/…` | **Yes** — `/ubuntu-security/` → `/ubuntu/` |

Real URLs used for verification:

```text
http://archive.ubuntu.com/ubuntu/dists/jammy/InRelease
http://archive.ubuntu.com/ubuntu/pool/main/g/gdbm/libgdbm6_1.23-1_amd64.deb
http://security.ubuntu.com/ubuntu/dists/jammy-security/InRelease
http://security.ubuntu.com/ubuntu/pool/main/a/accountsservice/accountsservice_22.07.5-2ubuntu1.6_amd64.deb
```

The remap on `/ubuntu-security/` means its `.deb` sub-location hits **Critical Fact #2**
(a nested regex `location` may not carry a URI on `proxy_pass`, so the parent's remap is
silently lost and must be re-applied with `rewrite … break`). This is the Fedora trap in
Debian clothing. `/ubuntu/` needs no rewrite, like `/debian/`.

### Pre-Identified Bug/Issue — Ubuntu's `s-maxage` beats our metadata TTL

Both Ubuntu upstreams send, on metadata **and** packages:

```text
Cache-Control: max-age=0, proxy-revalidate, s-maxage=3300
```

nginx's `ngx_http_upstream_process_cache_control` checks **`s-maxage=` first** and only
falls back to `max-age=`, and upstream `Cache-Control` **outranks** `proxy_cache_valid`.
So a naive copy of the Debian block would cache Ubuntu **metadata for 3300 s (55 min)**,
not the 60 s the block appears to say — a 55-minute window in which `InRelease` and
`Packages.gz` can disagree, which is exactly the apt *hash sum mismatch* failure this
project's TTL split exists to prevent.

Compare (same day, same method): `deb.debian.org` sends `max-age=120` and
`security.debian.org` `max-age=120` — both already short, which is why the existing
Debian metadata blocks are correct without intervention. Ubuntu is the outlier.

**Decision.** The Ubuntu metadata locations get `proxy_ignore_headers Cache-Control
Expires` **plus** an explicit `proxy_cache_valid 200 302 60s` and
`proxy_cache_revalidate on`.

This looks like it contradicts the existing in-config warning ("NEVER do this in the
metadata locations — there the upstream freshness signal is exactly what we want"). Read
that rule by its purpose: it forbids ignoring upstream freshness in order to cache
metadata **longer**. Here it is used to **clamp 3300 s down to 60 s** — the same
direction the rule is protecting. Constraint 2 in `CLAUDE.md` ("never lengthen metadata
TTL") is honored, not bent. The warning comment is amended to say *which* direction is
banned, so the next reader does not have to re-derive this.

`proxy-revalidate` is not implemented by nginx, so it cannot be relied on to save us here.

---

## Step-by-Step Tasks

### T1: `/ubuntu/` and `/ubuntu-security/` route blocks

**File**: `proxy/conf.d/pkgcache.conf`

**What to do**: Add two route blocks after the Debian ones, mirroring their structure,
with three Ubuntu-specific differences: the metadata `proxy_ignore_headers` clamp (see
Pre-Identified Bug), the `rewrite` in the `/ubuntu-security/` `.deb` sub-location, and an
amended comment on the `proxy_ignore_headers` rule explaining the allowed direction.
Update the `location = /` banner to list the new routes.

**After** (shape):
```nginx
location /ubuntu/ {
    proxy_pass http://archive.ubuntu.com/ubuntu/;
    proxy_cache pkg;
    proxy_ignore_headers Cache-Control Expires;   # clamp s-maxage=3300 DOWN to 60s
    proxy_cache_valid 200 302 60s;
    proxy_cache_valid 404 1m;
    proxy_cache_revalidate on;

    location ~* \.(deb|udeb)$ {
        proxy_pass http://archive.ubuntu.com;     # path already matches; no rewrite
        proxy_cache pkg;
        proxy_cache_valid 200 365d;
        proxy_ignore_headers Cache-Control Expires;
    }
}

location /ubuntu-security/ {
    proxy_pass http://security.ubuntu.com/ubuntu/;
    ...
    location ~* \.(deb|udeb)$ {
        rewrite ^/ubuntu-security/(.*)$ /ubuntu/$1 break;   # regex loc drops the remap
        proxy_pass http://security.ubuntu.com;
        ...
    }
}
```

**Verify**:
1. `RUNTIME=docker ./build && docker run --rm --entrypoint nginx iphands/pkgcache:latest -t`
   → `test is successful`
2. `RUNTIME=docker PORT=8080 CACHE_DIR=/tmp/pkgcache-test ./run` → `Up`, no `emerg` in logs
3. Both routes, metadata **and** `.deb`, twice each → `MISS` then `HIT`; a `404` on the
   security `.deb` means the `rewrite` is wrong
4. Metadata TTL is really 60 s, two ways: `valid_sec - date == 60` in the cache file
   header, and a third request after 65 s reports `REVALIDATED` (with the clamp missing it
   would still say `HIT` for 55 min)

### T2: `containers/ubuntu`

**Files**: `containers/ubuntu/{Dockerfile,pkgcache-setup,build,publish}`

**What to do**: Same shape as `containers/fedora` — `ARG CACHE` baked in, recorded as
`ENV PKGCACHE_URL`, `pkgcache-setup` installed to `/usr/local/bin` for re-pointing and
`--revert`, `build`/`publish` with the podman-preferred autodetect and `RUNTIME`/`IMAGE`
knobs. `ARG UBUNTU_RELEASE=22.04`.

apt specifics that differ from the Fedora script:
- rewrite the classic `sources.list` in place: `archive.ubuntu.com/ubuntu/` →
  `$CACHE/ubuntu/`, `security.ubuntu.com/ubuntu/` → `$CACHE/ubuntu-security/`
- match the **distro's own hostnames only**, never a blanket URL rewrite (pitfalls.md)
- also cover `ports.ubuntu.com`? **No** — that is the arm64/ppc64 archive and there is no
  route for it. Leave such lines untouched so they keep working from upstream.
- back up as `.pkgcache-bak`, once; `--revert` restores it
- `--no-makecache` skips `apt-get update` so the image builds with no network

**Verify**: `./build`, then `docker run --rm <image> apt-get install -y cowsay` completes;
`grep` the rewritten `sources.list`; `--revert` restores it byte-for-byte
(`cmp` against a pristine copy from the base image); a child image `FROM` it installs a
package through the cache.

### T3: Docs + knowledge harvest (Rule D)

**Files**: `README.md`, `CLAUDE.md`, `context/distilled.md`, `context/pitfalls.md`,
`context/plans/SERIES.md`

**What to do**: README route table + a `containers/ubuntu` usage block; CLAUDE.md route
table and layout tree; `distilled.md` gets the measured `Cache-Control` values for all
five upstreams and the nginx `s-maxage` precedence fact; `pitfalls.md` gets the
`s-maxage`-outranks-`proxy_cache_valid`-on-metadata entry; SERIES.md marks 06_3 done.

**Verify**: re-read each file after writing (Rule B2 §4 — a commit message is a factual
claim); `grep -c ubuntu README.md CLAUDE.md`.

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `proxy/conf.d/pkgcache.conf` | two route blocks + banner + amended ignore-headers comment | P0 |
| `containers/ubuntu/Dockerfile` | `ubuntu:22.04` + baked cache endpoint | P0 |
| `containers/ubuntu/pkgcache-setup` | classic `sources.list` surgery + `--revert` | P0 |
| `containers/ubuntu/build`, `publish` | image build/push pair | P1 |
| `README.md`, `CLAUDE.md` | route table, layout, usage | P1 |
| `context/distilled.md`, `context/pitfalls.md` | measured facts + the `s-maxage` trap | P1 |
| `scripts/fix-ubuntu` | host client fixer — **deliberately not in this plan** | P2 |

---

## Open Questions / Risks

1. **`s-maxage=3300` on metadata** (the big one). Mitigated by the clamp in T1 and
   verified two independent ways in T1 Verify 4. If the clamp is ever removed, apt hash
   mismatches return — hence the pitfalls.md entry.
2. **`archive.ubuntu.com` is round-robin DNS across many IPs.** nginx resolves by
   hostname with a `resolver` and a short TTL, so different requests may land on
   different mirror hosts. Harmless for correctness here — the cache key is the URI, not
   the peer, and the content is identical — but it means an upstream 5xx may be
   host-specific. `proxy_cache_use_stale` already covers that.
3. **`ports.ubuntu.com` (arm64) is not routed.** An arm64 client's `sources.list` points
   there and would bypass the cache entirely. T2 deliberately leaves those lines alone
   rather than pointing them at a route that does not exist. Recorded as a known
   limitation in README, not silently.
4. **`.ddeb` debug packages** live on `ddebs.ubuntu.com`, also unrouted. Same treatment.
5. **`--revert` fidelity.** The Fedora script's first cut got this wrong in a related way
   (`config-manager setopt` overrides outranking the file). Here the mechanism is a plain
   file copy, and T2 Verify uses `cmp` against a pristine copy from the base image rather
   than eyeballing it.

---

## Verification Checklist

- [ ] T1: `docker run --rm --entrypoint nginx iphands/pkgcache:latest -t` → `test is successful`
- [ ] T1: container `Up`, `docker logs pkgcache` free of `emerg`/`error`
- [ ] T1: `curl -sI localhost:8080/ubuntu/dists/jammy/InRelease` → `MISS` then `HIT`
- [ ] T1: `curl -sI localhost:8080/ubuntu/pool/main/g/gdbm/libgdbm6_1.23-1_amd64.deb` → `MISS` then `HIT`
- [ ] T1: `curl -sI localhost:8080/ubuntu-security/dists/jammy-security/InRelease` → `MISS` then `HIT`
- [ ] T1: `curl -sI localhost:8080/ubuntu-security/pool/main/a/accountsservice/accountsservice_22.07.5-2ubuntu1.6_amd64.deb` → `200`, `MISS` then `HIT` (**not** 404)
- [ ] T1: metadata cache-file header shows `valid_sec - date == 60`, and a request at +65 s → `REVALIDATED`
- [ ] T1: `.deb` cache-file header shows `valid_sec - date == 31536000` (365 d, not 3300 s)
- [ ] T2: `containers/ubuntu/build` succeeds; `shellcheck` clean on `build publish pkgcache-setup`
- [ ] T2: `apt-get update && apt-get install -y cowsay` inside the image succeeds through the cache
- [ ] T2: rewritten `sources.list` has no `archive.ubuntu.com` / `security.ubuntu.com` left
- [ ] T2: `pkgcache-setup --revert` → `cmp` equal to the base image's pristine `sources.list`
- [ ] T2: a child image `FROM iphands/pkgcache-ubuntu:22.04` installs a package through the cache
- [ ] T3: README/CLAUDE.md route tables list both Ubuntu routes; distilled.md and
      pitfalls.md contain the `s-maxage` finding (re-read, not assumed)
