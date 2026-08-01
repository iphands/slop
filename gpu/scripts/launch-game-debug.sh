#!/usr/bin/env bash
# launch-game-debug.sh — start Palworld with a profiling environment, capturing NOTHING yet.
#
# Flow this is built for:
#     ./scripts/launch-game-debug.sh          # launch; play to wherever you want to measure
#     ./scripts/record.sh                     # arm the capture
#     Ctrl-C                                  # stop
#
# The deferred start is genuine, not a trick: with INTEL_MEASURE's `control=` fifo set,
# Mesa initialises with capture DISABLED (src/intel/common/intel_measure.c:165-168) and
# only begins when a frame count is written to the fifo (:349-381). So the menus, the
# loading screen and the walk to your test spot cost nothing.
#
# PREREQUISITE (one time only). In Steam -> Palworld -> Properties -> Launch Options:
#     /home/merozas/prog/slop/gpu/scripts/proton-profile-wrapper.sh %command%
# Env cannot be injected into an already-running Steam any other way; see that script.

set -euo pipefail

APPID=1623730
REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
CAPTURES="$REPO_DIR/captures"
SESSION_ENV="$CAPTURES/session-env.sh"
SESSION_MODE="$CAPTURES/session-mode"
FIFO="$CAPTURES/measure.fifo"
WRAPPER="$REPO_DIR/scripts/proton-profile-wrapper.sh"
STAMP="$(date +%F_%H%M%S)"

mode="frame"
dx12=""
markers=0
no_fifo=0
batch_size=""

usage() {
    cat <<'EOF'
Usage: launch-game-debug.sh [--mode MODE] [--dx12] [--markers] [--no-fifo] [--batch-size N]

  --mode MODE   what record.sh will be able to capture (default: frame)
                  off     no Mesa instrumentation; MangoHud overlay only
                  frame   INTEL_MEASURE=type=frame   ~7200 rows / 2 min, negligible cost
                  rt      INTEL_MEASURE=type=rt      per render-target change
                  draw    INTEL_MEASURE=type=draw    HIGH COST - record.sh caps it at 10 frames
                  utrace  MESA_GPU_TRACES=print_json  richest; see the volume warning below
  --dx12        append -dx12 so the game runs D3D12/vkd3d-proton instead of D3D11/DXVK
  --markers     turn on UE5 pass-name markers (VKD3D_CONFIG=debug_utils + MESA_GPU_TRACES=markers)
  --no-fifo     drop INTEL_MEASURE's control= fifo: capture is LIVE FROM DRIVER INIT and
                record.sh only marks the window instead of arming. Use this when the fifo
                write itself crashes the game (see context/pitfalls.md) — writing to the
                fifo is the only thing record.sh does to the game process, and Palworld
                has aborted in vkQueueSubmit2 every time it was written.
  --batch-size N  INTEL_MEASURE snapshot slots per command buffer (driver default 65536).
                Each command buffer allocates N*8 bytes of mapped VRAM plus ~72*N host, so
                on a 4 GiB card the default is 512 KiB of VRAM per command buffer. 2048 is
                ample for frame/rt and is the cheap test of the allocation hypothesis.

The mode is fixed at LAUNCH, not at record time, because INTEL_MEASURE's type= is read
from the environment once at driver init. To change granularity, quit and relaunch.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mode)       mode="$2"; shift 2 ;;
        --dx12)       dx12="-dx12"; shift ;;
        --markers)    markers=1; shift ;;
        --no-fifo)    no_fifo=1; shift ;;
        --batch-size) batch_size="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "launch-game-debug: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

mkdir -p "$CAPTURES"

# --- sanity: is the wrapper actually wired into Steam's launch options? --------------
# This is the "a build that never loaded measures as a perfect no-op" failure mode from
# context/pitfalls.md, in launch-option form. Cheap to check, expensive to miss.
#
# Steam keeps one localconfig.vdf per account that has logged in on this box, and there
# are several here — most of them years stale. Taking `ls | head -1` picked a 2025 profile
# and reported the wrapper missing while it was set on the account actually in use. So:
# check every profile, and scope the match to Palworld's appid block rather than a bare
# grep, so launch options belonging to some other game cannot satisfy the check either.
wrapper_is_wired() {
    local f
    for f in "$HOME"/.local/share/Steam/userdata/*/config/localconfig.vdf \
             "$HOME"/.steam/steam/userdata/*/config/localconfig.vdf; do
        [[ -r $f ]] || continue
        awk -v appid="\"$APPID\"" -v needle="proton-profile-wrapper.sh" '
            $1 == appid && !inblk            { pending = 1; next }
            pending && $1 == "{"             { inblk = 1; depth = 1; pending = 0; next }
            inblk {
                if ($1 == "\"LaunchOptions\"" && index($0, needle)) { found = 1; exit }
                depth += gsub(/\{/, "{") - gsub(/\}/, "}")
                if (depth <= 0) inblk = 0
            }
            END { exit found ? 0 : 1 }
        ' "$f" && return 0
    done
    return 1
}

localconfig=$(ls -1 "$HOME"/.local/share/Steam/userdata/*/config/localconfig.vdf 2>/dev/null | head -1 || true)
if [[ -n $localconfig ]]; then
    if ! wrapper_is_wired; then
        cat >&2 <<EOF

  ┌─ LAUNCH OPTIONS NOT SET ────────────────────────────────────────────────────┐
  Steam's config does not mention proton-profile-wrapper.sh, so the profiling
  environment will NOT reach the game and every capture will come back empty.

  Set this once in Steam -> Palworld -> Properties -> Launch Options:

      $WRAPPER %command%

  (Steam writes localconfig.vdf lazily, so if you set it just now this warning
  can be stale. $CAPTURES/last-launch.log records what the wrapper actually
  applied — check it after launching.)
  └─────────────────────────────────────────────────────────────────────────────┘

EOF
        read -r -p "Continue anyway? [y/N] " ans
        [[ ${ans,,} == y ]] || exit 1
    fi
fi

# --- build the session environment ---------------------------------------------------
: >"$SESSION_ENV"
{
    echo "# generated by launch-game-debug.sh at $(date -Is) — mode=$mode"
    echo "MANGOHUD=1"
    # No autostart_log: logging is armed by hand so it lines up with record.sh.
    echo "MANGOHUD_CONFIG=fps,frametime,gpu_stats,gpu_temp,gpu_core_clock,gpu_mem_clock,cpu_stats,ram,vram,toggle_logging=Shift_L+F2,output_folder=$CAPTURES"
    # Identify which translation layer actually loads (Plan 01 T4) rather than assuming.
    echo "PROTON_LOG=1"
    echo "VKD3D_LOG_FILE=$CAPTURES/vkd3d_${STAMP}.log"
    echo "DXVK_LOG_LEVEL=info"
    echo "DXVK_LOG_PATH=$CAPTURES"
} >>"$SESSION_ENV"

measure_file=""
case "$mode" in
    off) ;;
    frame|rt|draw)
        measure_file="$CAPTURES/measure_${mode}_${STAMP}.csv"
        measure_opts="type=${mode}"
        if [[ -n $batch_size ]]; then
            measure_opts+=",batch_size=${batch_size}"
        fi
        # The control fifo is what record.sh writes to. Flipping intel_measure's global
        # `enabled` mid-flight is the one thing record.sh does to the game process, and it
        # is the only suspect for the vkQueueSubmit2 abort (context/pitfalls.md). --no-fifo
        # omits it so measurement is live from driver init and nothing transitions.
        if [[ $no_fifo -eq 1 ]]; then
            rm -f "$FIFO"
            FIFO=""
        else
            [[ -p $FIFO ]] || { rm -f "$FIFO"; mkfifo -m 0600 "$FIFO"; }
            measure_opts+=",control=${FIFO}"
        fi
        echo "INTEL_MEASURE=${measure_opts},file=${measure_file}" >>"$SESSION_ENV"
        ;;
    utrace)
        measure_file="$CAPTURES/utrace_${STAMP}.json"
        echo "MESA_GPU_TRACES=print_json" >>"$SESSION_ENV"
        echo "MESA_GPU_TRACEFILE=$measure_file" >>"$SESSION_ENV"
        # u_trace has NO control fifo — it records from driver init. Trim what we can.
        echo "INTEL_GPU_TRACEPOINT=-blit,-copy,+render_pass,+draw,+frame" >>"$SESSION_ENV"
        cat >&2 <<'EOF'

  NOTE: u_trace cannot be gated mid-session — unlike INTEL_MEASURE it has no control
  fifo, so it records from the moment the driver initialises, menus included. Keep the
  session short and expect a large file. record.sh will mark the window you cared about
  by timestamp so the rest can be trimmed in analysis.

EOF
        ;;
    *) echo "launch-game-debug: unknown mode '$mode'" >&2; usage >&2; exit 2 ;;
esac

if [[ $markers -eq 1 ]]; then
    # debug_utils is confirmed present in THIS build's d3d12core.dll (Proton 11.0,
    # proton-11.0-1b). The option name has moved between vkd3d-proton releases, so it was
    # read out of the shipped binary rather than from a blog post.
    echo "VKD3D_CONFIG=debug_utils" >>"$SESSION_ENV"
    echo "DXVK_DEBUG=markers" >>"$SESSION_ENV"
    if [[ $mode == utrace ]]; then
        sed -i 's/^MESA_GPU_TRACES=print_json$/MESA_GPU_TRACES=print_json,markers/' "$SESSION_ENV"
    fi
fi

# record what record.sh needs to know
{
    echo "mode=$mode"
    echo "fifo=$FIFO"
    echo "capture_file=$measure_file"
    echo "stamp=$STAMP"
    echo "dx12=${dx12:-no}"
    echo "markers=$markers"
} >"$SESSION_MODE"

# --- launch --------------------------------------------------------------------------
echo "launch-game-debug: mode=$mode markers=$markers ${dx12:+dx12 }-> $SESSION_ENV"
if [[ -n $measure_file ]]; then
    echo "launch-game-debug: capture will land in $measure_file"
fi
echo "launch-game-debug: launching appid $APPID ..."

# shellcheck disable=SC2086  # $dx12 is deliberately word-split (empty or -dx12)
steam -applaunch "$APPID" $dx12 >/dev/null 2>&1 &

cat <<EOF

  Game launching. Nothing is being captured yet.

    1. Play to the spot you want to measure.
    2. Run:  ./scripts/record.sh
    3. Ctrl-C to stop.

  Verify the environment actually reached the game:
      cat $CAPTURES/last-launch.log

EOF
