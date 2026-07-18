#!/bin/bash
# Launch the pkgcache STATS container on the host.
#
# Live copy: /main/docker/cache/create-stats.sh -- keep in sync with this one.
#
#   ./create-stats.sh
#
# Reads nginx's TSV access log, aggregates into SQLite, serves the dashboard.
# No shared network with the proxy -- the two are coupled only by the filesystem.

# Resolve spec beside this script, so the same file works from the repo checkout
# and from /main/docker/cache on the host without editing a hardcoded path.
cd "$(dirname "$0")" || exit 1
# shellcheck source=spec disable=SC1091
source ./spec

mkdir -p "$ROOT"/logs "$ROOT"/frontend
chown -R "${RUNAS%%:*}:${RUNAS##*:}" "$ROOT"/logs "$ROOT"/frontend

# --detach
podman run --replace --name "$STATS_NAME" \
  --user "$RUNAS" \
  -p "0.0.0.0:${STATS_PORT}:8081" \
  -v "$ROOT"/logs:/logs \
  -v "$ROOT"/frontend:/data \
  -v "$ROOT"/data:/cache:ro \
  "$STATS_PULL"

# Mounts, and why each is what it is:
#
#   /logs   rw  -- nginx writes here; the stats service reads AND PRUNES here.
#                  rw is required: the stats service is the only process that
#                  knows which files are fully ingested, so it is the only one
#                  that can delete them safely. It deletes a file only when it
#                  is older than LOG_RETENTION_DAYS *and* fully consumed *and*
#                  not today's or yesterday's -- that last condition is the
#                  margin for nginx's open_log_file_cache, which holds an fd for
#                  up to a minute after the last write. Unlink a file nginx still
#                  holds and it keeps appending to an unreachable inode, silently
#                  losing every request. See context/pitfalls.md.
#
#   /data   rw  -- the service's own scratch: stats.sqlite (+ -wal/-shm),
#                  .ingest.lock, labels.json.
#
#   /cache  ro  -- the nginx package cache, mounted READ-ONLY and used ONLY for
#                  a statvfs and a size walk of pkg/, so the dashboard can show
#                  "38.4 GB / 100 GB". Package content is never read.
#
# RUNAS must match the proxy's exactly. If it doesn't, nginx writes logs this
# container cannot read and the dashboard shows zeros with no error anywhere --
# which is why the service logs a loud ERROR on EACCES and surfaces
# ingest.logs_readable in its payload.
