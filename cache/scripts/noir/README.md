# `scripts/noir/` — the live host's launch recipe

These are the **host-side** deployment scripts for `noir`, the container host that runs the
production cache. They are version-controlled here so the recipe is reviewable and survives
a host rebuild — previously they existed **only** at `/main/docker/cache/` on the box.

> **Not to be confused with `scripts/fix-*`**, one level up. Those are *client* scripts you
> copy onto a Debian/Fedora machine to point its package manager at the cache. These are
> *server* scripts that launch the containers. Different machines, different audiences.

## Files

| File | Purpose |
|---|---|
| `spec` | Names, images, ports, uid — sourced by both scripts |
| `create.sh` | Launch the proxy container (`cacher`) |
| `create-stats.sh` | Launch the stats/dashboard container (`cacher-stats`) |

## Deploying

The scripts resolve `spec` **beside themselves**, so they work from a repo checkout or from
`/main/docker/cache/` without editing a hardcoded path:

```bash
# on noir, as root
rsync -a scripts/noir/ /main/docker/cache/
cd /main/docker/cache
./create.sh
./create-stats.sh
```

**Keep the two copies in sync, and change this one first.** A change made only on the host
is invisible to review and lost on a rebuild.

## Host layout

```
/main/docker/cache/
├── spec  create.sh  create-stats.sh
├── data/       → proxy :/var/cache/nginx  rw    nginx's package cache
│                → stats :/cache            ro    statvfs + pkg/ size ONLY
├── logs/       → proxy :/logs              rw    nginx writes the TSV stats log
│                → stats :/logs             rw    reads AND prunes consumed files
└── frontend/   → stats :/data              rw    stats.sqlite, .ingest.lock, labels.json
```

Three directories, one purpose each. nginx's cache manager (`max_size=100g`,
`inactive=365d`) walks only `data/pkg/`, so it can never see the logs or the database.

## Things that will bite you

- **Both containers must run as the same uid:gid, launched the same way.** nginx writes the
  logs; the stats service reads them. A mismatch means the stats container cannot read what
  nginx wrote, and **the failure mode is a dashboard of silent zeros, not an error**. The
  stats service logs a loud `ERROR` on `EACCES` and reports `ingest.logs_readable: false`
  precisely so this is diagnosable.
- **`logs/` is `rw` to the stats container on purpose.** It is the only process that knows
  which files are fully ingested, so it is the only one that can prune them safely. A host
  `find -mtime +N -delete` would eventually delete data that was never read.
- **`data/` is `ro` to the stats container** and used only for a `statvfs` plus a size walk
  of `pkg/`. It never reads package content.
- **`mkdir` + `chown` before launching is not optional.** nginx will not create the log
  directory itself (the `access_log` path contains a variable), and a missing directory
  produces one `error_log` line per request plus zero stats.
- **The host runs podman and has no docker.** The repo's `build`/`run`/`publish` prefer
  podman for this reason — don't "fix" them to prefer docker. Use `RUNTIME=docker` on a dev
  machine instead.
- `-e APP_UID` / `-e APP_GID` on the podman command line are **no-ops** — the image never
  reads them, only `--user` has any effect. They are meaningful only to the repo's `./run`,
  which uses them host-side to chown volumes.
- `--detach` is commented out in both scripts, matching the existing host convention. Add it
  (and a `--restart` policy) if you want them supervised rather than foreground.
