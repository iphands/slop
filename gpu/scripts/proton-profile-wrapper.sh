#!/usr/bin/env bash
# proton-profile-wrapper.sh — the shim Steam launches the game through.
#
# This exists for one reason: Steam is normally ALREADY RUNNING when we want to profile.
# `env FOO=bar steam -applaunch <id>` does not work in that case — `-applaunch` is an IPC
# message to the running client, and the game inherits the *client's* environment, not the
# shell's. So the profiling environment has to be injected at the point Steam spawns the
# game. That point is the launch options, and this is the script they point at.
#
# Set ONCE in Steam -> Palworld -> Properties -> Launch Options:
#
#     /home/merozas/prog/slop/gpu/scripts/proton-profile-wrapper.sh %command%
#
# After that it is inert: with no session env file it execs the game unchanged, so it is
# safe to leave in place between profiling sessions. scripts/launch-game-debug.sh writes
# the session env file; scripts/record.sh arms the capture.

set -uo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
SESSION_ENV="$REPO_DIR/captures/session-env.sh"
LAUNCH_LOG="$REPO_DIR/captures/last-launch.log"

mkdir -p "$REPO_DIR/captures"

{
    echo "=== proton-profile-wrapper $(date -Is) ==="
    echo "cwd: $PWD"
    echo "argv[0..2]: ${1:-} ${2:-} ${3:-}"
} >"$LAUNCH_LOG" 2>&1

if [[ -r $SESSION_ENV ]]; then
    # shellcheck source=/dev/null
    set -a; source "$SESSION_ENV"; set +a
    {
        echo "applied session env from: $SESSION_ENV"
        echo "--- profiling vars actually exported into the game process ---"
        for v in INTEL_MEASURE INTEL_DEBUG INTEL_GPU_TRACEPOINT MESA_GPU_TRACES \
                 MESA_GPU_TRACEFILE MANGOHUD MANGOHUD_CONFIG VKD3D_CONFIG VKD3D_LOG_FILE \
                 DXVK_DEBUG DXVK_LOG_LEVEL DXVK_LOG_PATH PROTON_LOG VK_DRIVER_FILES; do
            [[ -n ${!v:-} ]] && echo "  $v=${!v}"
        done
        echo "-------------------------------------------------------------"
    } >>"$LAUNCH_LOG" 2>&1
else
    echo "no session env file at $SESSION_ENV — launching unmodified" >>"$LAUNCH_LOG"
fi

echo "exec: $*" >>"$LAUNCH_LOG"
exec "$@"
