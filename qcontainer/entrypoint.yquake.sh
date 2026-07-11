#!/bin/bash
set -euo pipefail

# Optimized Yamagi Quake II dedicated server.
#
# Mount the retail game data (pak files) read-only at:
#     /usr/share/games/quake2/baseq2
# Optionally mount a server config at:
#     /usr/share/games/quake2/baseq2/server.cfg
# which is exec'd on boot (a missing server.cfg is harmless).
#
# Extra args passed to `podman run` / `docker run` are forwarded to q2ded, e.g.:
#     ... iphands/quake2:yquake +set deathmatch 1 +map q2dm1

cd /opt/yquake2

exec ./q2ded \
    +set dedicated 1 \
    +set game baseq2 \
    +exec server.cfg \
    "$@"
