#!/bin/bash
set -euo pipefail

# Optimized Q2REPRO (Paril's Q2PRO re-release fork) dedicated server.
#
# Mount the retail game data (pak files) read-only at:
#     /usr/share/games/quake2/baseq2
# Optionally mount a server config at:
#     /usr/share/games/quake2/baseq2/server.cfg
# which is exec'd on boot (a missing server.cfg is harmless).
#
# The game library is loaded from the image (/usr/lib*/q2repro/baseq2), so the
# read-only data mount only needs the pak files, not a game .so.
#
# Extra args passed to `podman run` / `docker run` are forwarded to q2reproded:
#     ... iphands/quake2 +set deathmatch 1 +map q2dm1

exec /usr/bin/q2reproded \
    +set dedicated 1 \
    +set basedir /usr/share/games/quake2 \
    +set homedir /opt/q2repro \
    +exec server.cfg \
    "$@"
