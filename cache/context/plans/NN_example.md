# Plan NN — [Title]

> **Status**: pending
> **Created**: YYYY-MM-DD
> **Depends on**: Plan N | N/A
> **Goal**: One-sentence deliverable description.
> **Agent**: implementation agent (ralph-loop) | sub-agent | etc.

---

> **Before writing any config, re-read `context/plans/RULES.md` in full** — especially
> Rule A (prove it runs and caches; there is no compiler here).
> For historical context, completed plans live in `context/plans/completed/`.

<!--
  CANONICAL TEMPLATE. Copy to NN_name.md using the next number from SERIES.md,
  create the paired NN_name_tracker.md, and fill in EVERY section. Delete the
  instructional comments as you go. Do not delete section headings — if a section
  genuinely does not apply, write "N/A — <reason>" under it.
-->

## TL;DR

**What**: One sentence describing what is being done.

**Deliverables**:
1. Concrete output one (a file, a route block, a script)
2. Concrete output two

**Estimated effort**: Small (2 h) | Small–Medium (half day) | Medium (1 day) | Large (3 days)

---

## Context

Background, rationale, prior findings, decisions already made. Cite `context/distilled.md`
and `context/pitfalls.md` entries you relied on.

### Pre-Identified Bug/Issue

<!-- Confirmed problems documented BEFORE work starts. Include the observed symptom
     verbatim (error text, curl output, apt/dnf message) — not a paraphrase. -->

### Why [Approach]

<!-- Justify the design choice against the alternative you rejected. -->

### Key Facts

<!-- MANDATORY for any plan touching routes. Record REAL, FETCHED URLs — one metadata
     file and one package file — for every upstream involved. Guessed path shapes are
     the #1 cause of broken route blocks. Prove each with a bare curl to the UPSTREAM,
     before writing any nginx:

       curl -sI https://upstream.example/path/to/repomd.xml   -> 200
       curl -sI https://upstream.example/path/to/pkg-1.0.rpm  -> 200

     Then record: client path prefix, upstream base URL, upstream path prefix (do they
     differ? that's the rewrite), transport (HTTP/HTTPS), and which file extensions are
     the immutable package files. -->

| Fact | Value | How confirmed |
|---|---|---|
| Client path prefix | `/<distro>/` | design |
| Upstream base | `https://…` | `curl -sI` on YYYY-MM-DD |
| Upstream path prefix | `/…` (differs? ⇒ needs `rewrite` in regex sub-locations) | `curl -sI` |
| Metadata files | `repomd.xml`, … | fetched |
| Package files (immutable) | `*.rpm` | fetched |
| GPG key path on client | `/etc/pki/…` | on-host check |

---

## Step-by-Step Tasks

### T1: [Task title]

**File**: `conf.d/pkgcache.conf`

**What to do**: Detailed instructions.

**Before**:
```nginx
# old config
```

**After**:
```nginx
# corrected config
```

**Verify**:
```bash
export RUNTIME=docker    # this machine has docker; noir.lan has podman
./build && docker run --rm --entrypoint nginx iphands/pkgcache:latest -t
PORT=8080 CACHE_DIR=/tmp/pkgcache-test ./run && sleep 2 && docker logs pkgcache
curl -sI http://localhost:8080/<path> | grep -i x-cache-status   # MISS
curl -sI http://localhost:8080/<path> | grep -i x-cache-status   # HIT
```

**Commit**: `task(T1): <short description>` — *commit before marking done (Rule B).*

### T2: [Task title]

<!-- … same shape … -->

---

## Critical Files

| File | Change | Priority |
|------|--------|----------|
| `conf.d/pkgcache.conf` | Add the `/<distro>/` route + TTL split | P0 |
| `scripts/fix-<distro>` | New client fixer with `--revert` | P1 |
| `README.md` | Document the new route + client setup | P1 |
| `context/distilled.md` | Record the confirmed upstream path shape | P2 |

Priority values: `P0` = blocking, `P1` = important, `P2` = nice-to-have.

---

## Open Questions / Risks

1. **[Risk]** — *Mitigation:* […]
2. **[Open question]** — *How we'll settle it:* […]

<!-- Recurring risks worth copying in when relevant:
     - Nested regex `location` drops the proxy_pass URI ⇒ path remap silently lost
       (see pitfalls.md). Mitigation: fetch a real package file through the cache.
     - Metadata cached too long ⇒ hash-mismatch errors on clients. Mitigation: keep 60s
       + proxy_cache_revalidate; verify with two refreshes spanning >60s.
     - Hammering an upstream master mirror. Mitigation: prefer a regional mirror.
     - Client fixer edits a third-party repo file. Mitigation: match on the distro's own
       hostnames only; back up every file touched. -->

---

## Verification Checklist

<!-- One checkbox per task. Each must be a testable assertion with an OBSERVABLE output.
     "Config looks correct" is not an assertion. -->

- [ ] T1: `nginx -t` in the built image prints `test is successful`
- [ ] T1: container is `Up` after `./run`; `docker logs` shows no `emerg`/`error`
- [ ] T1: metadata URL → `MISS` then `HIT` on repeat
- [ ] T1: **package** URL (regex sub-location) → 200 + `MISS` then `HIT` — *this is the
      one that catches a lost path remap*
- [ ] T2: `shellcheck scripts/fix-<distro>` clean
- [ ] T2: fixer runs on a real client; the distro's refresh command succeeds
- [ ] T2: `--revert` restores originals; refresh succeeds again
- [ ] All: `README.md` updated in the same commit as the behavior change
- [ ] All: findings harvested into `distilled.md` / `pitfalls.md` (Rule D)
- [ ] All: plan + tracker `git mv`'d to `completed/`, `SERIES.md` marked done (Rule C)
