#!/bin/bash
# Launch the pkgcache PROXY container on the host.
#
# Live copy: /main/docker/cache/create.sh -- keep in sync with this one.
#
#   ./create.sh
#
# Three host directories, one purpose each:
#   data/      nginx's package cache      (this container: rw)
#   logs/      the stats TSV access log   (this container: rw, stats: rw)
#   frontend/  the stats service scratch  (this container: not mounted)

# shellcheck source=spec disable=SC1091
source /main/docker/cache/spec

# ---- provision the directories nginx needs ---------------------------------
# nginx will NOT create the log directory itself -- the access_log path contains
# a variable ($logdate), and a missing directory produces one error_log line per
# request plus zero stats. Both dirs must be owned by RUNAS.
mkdir -p "$ROOT"/data "$ROOT"/logs
chown -R "${RUNAS%%:*}:${RUNAS##*:}" "$ROOT"/data "$ROOT"/logs

# --detach
podman run --replace --name "$NAME" \
  --user "$RUNAS" \
  -p "0.0.0.0:${PORT}:8080" \
  -v "$ROOT"/data:/var/cache/nginx \
  -v "$ROOT"/logs:/logs \
  "$PULL"

# NOTE: the old  -e APP_UID=1000 -e APP_GID=1000  were no-ops -- the image never
# reads them; only --user has any effect. They are dropped here rather than kept
# as decoration. (APP_UID/APP_GID *are* meaningful to the repo's ./run script,
# which uses them host-side to chown the volume.)
