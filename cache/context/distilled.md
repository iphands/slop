# Distilled — confirmed nginx-caching & package-manager facts

Compact learnings for pkgcache. Read before new work. Append new findings; keep it dense.

**Provenance discipline:** every entry is tagged with how it is known.
`[SOURCE]` = nginx/apt/dnf documented behavior. `[REPO]` = asserted by this repo's config
and comments. `[LIVE]` = observed against a running cache — include the date.
**Do not upgrade a tag without doing the work.**

---

## nginx `proxy_pass` path rewriting — the rule that shapes `proxy/conf.d/pkgcache.conf` [SOURCE][LIVE 2026-07-18]

- **Prefix location, `proxy_pass` WITH a URI part** → nginx replaces the matched location
  prefix with that URI:
  ```nginx
  location /fedora/ { proxy_pass https://dl.fedoraproject.org/pub/fedora/; }
  # client /fedora/linux/x → upstream /pub/fedora/linux/x
  ```
- **Prefix location, `proxy_pass` with NO URI part (bare host)** → the full original
  request URI is passed through unchanged:
  ```nginx
  location /debian/ { proxy_pass http://deb.debian.org; }
  # client /debian/pool/x → upstream /debian/pool/x   (upstream path happens to match)
  ```
- **Regex location** (`location ~* \.rpm$`) → **`proxy_pass` MUST NOT have a URI part.**
  nginx rejects it at config-parse time. **Verified live 2026-07-18** against
  `nginxinc/nginx-unprivileged:1.27-alpine`, exact text:
  ```text
  [emerg] "proxy_pass" cannot have URI part in location given by regular expression,
  or inside named location, or inside "if" statement, or inside "limit_except" block
  ```
  So a nested regex location **cannot inherit its parent prefix location's path remap** —
  you must redo it explicitly:
  ```nginx
  location ~* \.rpm$ {
      rewrite ^/fedora/(.*)$ /pub/fedora/$1 break;   # re-apply the remap by hand
      proxy_pass https://dl.fedoraproject.org;       # bare host, no path
  }
  ```
  `break` stops rewrite processing and keeps the request in this location.

**Consequence for this repo:** Debian's regex sub-locations need no `rewrite` (client path
`/debian/…` already equals the upstream path); Fedora's does (client `/fedora/…` →
upstream `/pub/fedora/…`). Any new upstream whose path prefix differs from the client
prefix needs the same treatment. See `context/pitfalls.md`.

## Logging variables — measured, not assumed [LIVE 2026-07-18]

Probed against `nginxinc/nginx-unprivileged:1.27-alpine` with this repo's Fedora/Debian
location blocks reproduced.

- **`$uri` is post-`rewrite`; `$request_uri` is the original.** They diverge **only** where
  a `rewrite` fired — here, only `.rpm`:
  ```text
  metadata: uri=[/fedora/…/repomd.xml]        request_uri=[/fedora/…/repomd.xml]      agree
  .rpm:     uri=[/pub/fedora/…/x-1.0.rpm]     request_uri=[/fedora/…/x-1.0.rpm]       DIVERGE
  .deb:     uri=[/debian/…/x.deb]             request_uri=[/debian/…/x.deb]           agree
  ```
  **Always log `$request_uri`.** See `pitfalls.md`.
- **`$request_uri` preserves percent-encoding** — `usbmuxd-1.1.1%5e2025….rpm` stays
  encoded (confirmed in production logs too). A path classifier must not assume decoded
  text, and two clients encoding differently would produce two distinct rows.
- **`$upstream_bytes_received` can EXCEED `$body_bytes_sent`.** Measured on a MISS: served
  5966, upstream 6499 — it counts **response headers**, `$body_bytes_sent` does not. So a
  whole-window `Σ served − Σ upstream` understates savings by every MISS's header overhead
  and can go negative. Correct formula:
  ```
  bytes_saved = Σ over {HIT,REVALIDATED,STALE,UPDATING} of max(0, body_bytes − upstream_bytes)
  ```
  This is right for all three cases: HIT (upstream 0 → full body), REVALIDATED (nets out
  the ~300-byte 304 round-trip), MISS (contributes 0, not a negative).
- `$upstream_bytes_received` is `-` on a HIT; `$upstream_cache_status` is `-` on
  non-proxied locations (`location = /`).
- **A HEAD request logs `body_bytes_sent=0` with a real cache status** (`HEAD 200 0 - HIT`).
  Your own `curl -sI` verification traffic therefore inflates hit *counts* while
  contributing zero bytes — exclude HEAD from byte ratios.
- `$msec` is epoch-with-milliseconds (`1784416726.694`). **The ingest never infers time
  from a filename** — `$time_iso8601` is local-with-offset and is used only to name the
  dated log file.
- **A variable in the `access_log` path requires an existing `root`** or nginx silently
  writes nothing. See `pitfalls.md` — this is the highest-deception failure found so far.
- `log_format … escape=default` with 9 tab-separated fields held its framing across HIT,
  MISS, 404, HEAD, the banner location, and a `%09%22` URI. Framing is robust *because*
  `$request_uri` keeps percent-encoding — a raw tab cannot appear in a valid request line.

## The live deployment [REPO — operator-confirmed 2026-07-18]

`noir:/main/docker/cache/`, launched by `spec` + `create*.sh` (mirrored in the repo's
`scripts/noir/`). **podman, no docker.** Containers are `cacher` and `cacher-stats`.

| Host dir | proxy | stats | Purpose |
|---|---|---|---|
| `data/` | `:/var/cache/nginx` **rw** | `:/cache` **ro** | nginx package cache; stats reads *sizes only* |
| `logs/` | `:/logs` **rw** | `:/logs` **rw** | nginx writes the TSV log; stats reads **and prunes** |
| `frontend/` | — | `:/data` **rw** | `stats.sqlite`, `.ingest.lock`, `labels.json` |

- Three top-level dirs, one purpose each. nginx's cache manager walks only `data/pkg/`, so
  `max_size=100g` can never see the logs or the DB.
- **`logs/` is rw to stats deliberately.** The stats service is the only process that knows
  which files are fully ingested, so it is the only one that can prune safely. A host cron
  doing `find -mtime +N -delete` would delete un-ingested data silently.
- **`-e APP_UID` / `-e APP_GID` on the podman command line are no-ops** — the image never
  reads them; only `--user` has any effect. They *are* meaningful to the repo's `./run`,
  which uses them host-side to chown volumes.
- Ports: proxy `3129→8080`, stats `3130→8081`.
- Both containers must use the **same uid:gid and the same launch flags**, or nginx writes
  logs stats cannot read — and the symptom is a dashboard of zeros with no error.

## Real traffic shape [LIVE 2026-07-18 — operator-supplied production logs]

Two samples from the live cache (`noir.lan`), one apt run and one dnf run. These are the
numbers the dashboard has to render well.

- **Multi-client is real**: `192.168.10.10` (Debian 13), `192.168.10.99` (Fedora 44).
- **apt/dnf fetch in bursts.** 32 packages arrived in the **same second**, all HIT. Two
  consequences: nginx's atomic single-`write()` per line genuinely matters (32 lines from
  multiple workers in one second), and an hourly bucket can be one 3-second spike, so a
  "requests per hour" chart is spiky by nature — not a bug to smooth away.
- **50.7 MiB saved in that one second**, all from cache.
- **Package sizes span ~9,663×** — 1,468 B (`linux-image-amd64`, a metapackage) to 14.2 MB
  (`mesa-vulkan-drivers`). **One package was 27% of the entire run's bytes.** This is the
  concrete argument for byte-ratio over request-ratio, *and* the reason a linear bar chart
  of package sizes renders 30 of 32 rows as invisible slivers.
- **Percent-encoding is pervasive, not exotic**: 42 instances of `%2b` (`+`) in those 32
  URIs, because Debian versions are full of `+` (`18.2.7+ds-1+deb13u1`,
  `2.12.7+dfsg+really2.9.14-2.1+deb13u3`). Fedora uses `%5e` (`^`). Both emit **lowercase**
  hex. **Percent-decode before storing a path**, or the UI is unreadable and any client
  encoding differently creates a duplicate row for the same package.
- **`/debian-security/` really is a separate first path segment**
  (`/debian-security/pool/updates/main/…`), so "repo = first path segment" classifies it
  correctly with no special case.
- **Package filename parsing** (validated against the real names above):
  - `.deb` → split on `_`: `name_version_arch.deb`. Versions contain `-` and `+` freely but
    never `_`, which is what makes this safe. `python3-idna_3.10-1+deb13u1_all.deb` → arch
    can be `all`.
  - `.rpm` → split from the **right**: `.rpm`, then `.arch`, then `-release`, then
    `-version`; the remainder is the name. Splitting from the left is wrong — rpm names
    contain `-` (`pipewire-jack-audio-connection-kit-libs-1.6.8-1.fc44.x86_64.rpm`).

## Cache key & TTL precedence [SOURCE]

- **Default cache key** is `$scheme$proxy_host$request_uri`. `$request_uri` is the
  **original, pre-`rewrite`** client URI — so the prefix location and its nested regex
  location produce the **same key** for the same client URL. That is what makes the
  split-by-extension design work.
- **TTL precedence, highest first:**
  1. `X-Accel-Expires` (upstream)
  2. `Cache-Control` / `Expires` (upstream)
  3. `proxy_cache_valid` (our config)

  ⚠️ **`proxy_cache_valid` is the LOWEST priority.** If an upstream sends its own
  `Cache-Control: max-age=…`, it **wins** over our `proxy_cache_valid 200 365d`.
  Overriding requires `proxy_ignore_headers Cache-Control Expires;`.
  **This repo does not set `proxy_ignore_headers`** — see the open question below.
- `proxy_cache_revalidate on` → on expiry, revalidate with `If-Modified-Since` /
  `If-None-Match` instead of re-downloading. A `304` is nearly free. This is what makes a
  60 s metadata TTL cheap.
- `inactive=` (in `proxy_cache_path`) is **eviction by non-access**, orthogonal to TTL: an
  entry not *read* within the window is deleted even if still fresh. `max_size` is the
  disk cap enforced by the cache-manager process.

## Debian / apt [SOURCE][REPO]

- `deb.debian.org` is a **Fastly CDN** alias; `security.debian.org` serves
  `/debian-security/`. Both over plain **HTTP**.
- Repo layout: metadata under `dists/<suite>/…` (`InRelease`, `Release`, `Packages*`,
  `by-hash/…`); packages under `pool/<component>/<letter>/<src>/<file>.deb`.
- **`by-hash/` matters:** modern apt fetches indices by content hash. Those URLs are
  effectively immutable *per hash*, but the `Release`/`InRelease` that names them is not —
  which is exactly why serving stale `InRelease` produces
  `Hash Sum mismatch` / `File has unexpected size`.
- **Two sources formats coexist:** legacy one-line `deb http://… suite comps` in
  `sources.list`/`*.list`, and **deb822** multi-line `Types:/URIs:/Suites:/Components:/Signed-By:`
  in `*.sources`. Debian 13 ships deb822 (`/etc/apt/sources.list.d/debian.sources`).
  `scripts/fix-debian` rewrites URLs in **both** with one `sed`, which works because it
  matches on the URL, not the file format.
- **GPG:** `Signed-By:` pins the keyring. Signatures are verified after download ⇒ HTTP
  transport to the cache is safe. Never disable.

## Fedora / dnf [SOURCE][REPO]

- **The metalink problem:** stock `fedora`/`updates` repos use `metalink=` — dnf asks
  `mirrors.fedoraproject.org` for a mirror list, then fetches over **HTTPS from a
  dynamically chosen mirror**. A forward proxy sees only a `CONNECT` tunnel (uncacheable),
  and the varying mirror means unstable cache keys. **This is the whole reason pkgcache is
  a reverse cache.**
- **The fix pattern:** define explicit `baseurl=` repos pointing at the cache and
  **disable the stock metalink repos**. That is precisely what `scripts/fix-fedora` does.
- Master mirror path shape:
  `https://dl.fedoraproject.org/pub/fedora/linux/releases/<ver>/Everything/<arch>/os/`
  and `…/pub/fedora/linux/updates/<ver>/Everything/<arch>/`. Metadata lives under
  `repodata/` (`repomd.xml` + hash-named `*.xml.gz`); packages under `Packages/<letter>/`.
- **`repomd.xml` is the metadata root** and the one file that must never be stale — the
  other metadata files are content-hash-named (immutable per name), so a stale
  `repomd.xml` points at files that may already be gone upstream ⇒
  `Failed to download metadata`.
- **dnf5 vs dnf4 CLI drift:** enabling/disabling a repo is
  `dnf config-manager setopt <repo>.enabled=0` on dnf5, but
  `dnf config-manager --set-disabled <repo>` on dnf4. `scripts/fix-fedora`'s
  `set_repo_enabled()` tries the former and falls back to the latter — copy that helper
  for any new RPM-distro fixer.
- `$releasever` / `$basearch` are expanded by dnf inside `.repo` files — they must be
  **escaped** (`\$releasever`) when written from a shell heredoc.

## Rootless container facts [REPO]

- `nginxinc/nginx-unprivileged` listens on **8080**, not 80, and ships a stock
  `/etc/nginx/conf.d/default.conf` that **also** binds 8080 — it is deleted in the
  proxy/Dockerfile or it shadows ours. **[LIVE 2026-07-18]** Two `server` blocks on `:8080`
  both with `server_name _` is **not** a config error — `nginx -t` passes clean and the
  first one loaded silently becomes the default server. The failure is invisible to
  validation; only a real request reveals it.
- **[LIVE 2026-07-18]** This repo's `proxy/nginx.conf` + `proxy/conf.d/pkgcache.conf` pass
  `nginx -t` (`syntax is ok` / `test is successful`) on
  `nginxinc/nginx-unprivileged:1.27-alpine`.
- Every writable path must be under the mounted volume or `/tmp`: `pid /tmp/nginx.pid`,
  all `*_temp_path` under `/var/cache/nginx`. `use_temp_path=off` on `proxy_cache_path`
  avoids a cross-device rename into the bind mount.
- nginx creates temp dirs with a **single-level `mkdir()`** — hence one level under the
  volume root, not a deep path, on a fresh empty mount.
- `resolver` is **mandatory** when upstreams are named by hostname in `proxy_pass`.
  `127.0.0.11` = docker/podman embedded DNS; `1.1.1.1` = fallback for host/bridge networks.
- **Rootless podman:** host `APP_UID` ≠ in-container uid without
  `--userns=keep-id:uid=…,gid=…`. docker and rootful podman need no flag.
- **Engine split [LIVE 2026-07-18]:** the dev machine has **docker** (its rootless podman
  is broken — `podman system migrate`); the live host `noir.lan` has **podman and no
  docker**. Verify with `RUNTIME=docker`; leave the scripts preferring podman so they work
  unchanged in production. `--userns=keep-id` is therefore **not testable on the dev
  machine** — it is rootless-podman-only.
- The alpine base has **busybox `wget`, no `curl`** — the HEALTHCHECK uses `wget`.

---

## Upstream `Cache-Control` vs our TTLs — RESOLVED, and it was overriding us [LIVE 2026-07-18]

Measured by reading `valid_sec` out of the nginx cache-file header rather than trusting the
config. Layout: `ngx_http_file_cache_header_t` starts with `version` (`ngx_uint_t`, 8 bytes
on 64-bit), then `valid_sec` — a `time_t` at **offset 8**, little-endian, holding an
**absolute unix expiry**:

```bash
python3 -c "import struct,time; d=open(F,'rb').read(16); \
  print(struct.unpack('<q', d[8:16])[0] - int(time.time()))"   # seconds to expiry
```

| Upstream | Sends | TTL actually applied | Configured |
|---|---|---|---|
| `deb.debian.org` packages | `Cache-Control: public, max-age=2592000` | **30 d** | 365 d |
| `deb.debian.org` metadata | `Cache-Control: public, max-age=120` | **120 s** | 60 s |
| `security.debian.org` metadata | `max-age=120` + `Expires:` | **120 s** | 60 s |
| `dl.fedoraproject.org` (both) | **nothing** | 365 d / 60 s ✓ | as configured |

So every Debian package had been cached for 30 days, not the year the config claimed —
since day one. Fedora was unaffected, which is exactly why nothing looked wrong.

**Fix applied:** `proxy_ignore_headers Cache-Control Expires;` in the three **package**
sub-locations only. Re-measured on a cold cache: `.deb` **365.0 d** (was 30.0), `.rpm`
still 365.0 d, Debian metadata still **119 s**.

**Metadata deliberately still defers to upstream.** 120 s is Debian's own considered value
for their own CDN, `proxy_cache_revalidate` is on, and overriding a freshness signal would
trade correctness for a hit-ratio number nobody asked for.

**General rule: `proxy_cache_valid` is a fallback, not an instruction.** Any claim about
retention must be *measured from the cache file*, never read off the config.

## Remaining Open Questions

1. **Are the `by-hash/` metadata URLs caught by the metadata TTL?** They match the parent
   prefix location (no extension match), so they should be — confirm by fetching one and
   checking `X-Cache-Status` plus retention.
2. ~~Does `X-Cache-Status` appear on responses from the nested regex locations?~~
   **RESOLVED [LIVE 2026-07-18]: yes.** A real `.deb` and a real `.rpm` fetched through the
   cache both return `X-Cache-Status: HIT` — the server-level `add_header` is inherited
   because the sub-locations declare none of their own.
