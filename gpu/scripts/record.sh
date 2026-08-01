#!/usr/bin/env bash
# record.sh — arm the capture on an already-running game, stop it with Ctrl-C.
#
# Run this once you are standing where you want to measure. It:
#   1. finds the running game process,
#   2. arms INTEL_MEASURE through its control fifo  (echo N  -> capture starts),
#   3. samples engine busy / VRAM / clocks / throttle via scripts/gpu-survey.sh,
#   4. on Ctrl-C, disarms the fifo (echo 0 -> capture stops) and prints a run verdict.
#
# Why Ctrl-C can stop a frame-counted capture: intel_measure.c:372-377 treats a written
# 0 as "disable now", overriding the frame count. So we arm with a deliberately huge N
# and revoke it on exit, which gives open-ended capture with a clean stop.

set -euo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
CAPTURES="$REPO_DIR/captures"
SESSION_MODE="$CAPTURES/session-mode"
GAME_MATCH="Palworld-Win64-Shipping.exe"
STAMP="$(date +%F_%H%M%S)"

# INTEL_MEASURE counts frames, so "until I say stop" is spelled "a number you will never
# reach". Except for type=draw, which is capped hard — see context/pitfalls.md.
OPEN_ENDED_FRAMES=100000000
DRAW_FRAME_CAP=10

interval=0.2
label=""
draw_frames=$DRAW_FRAME_CAP
no_arm=0

usage() {
    cat <<'EOF'
Usage: record.sh [--interval SEC] [--label TEXT] [--draw-frames N] [--no-arm]

  --interval SEC    sampler period (default 0.2 = 5 Hz)
  --label TEXT      free-text note stored in the CSV header (e.g. "base at dusk, 12 pals")
  --no-arm          run the sampler ONLY; never write the control fifo. The fifo write is
                    the sole thing this script does to the game process, so this is the
                    safe mode while the vkQueueSubmit2 abort is unexplained.
  --draw-frames N   frames to capture in draw mode (default 10; raising this is how you
                    turn a 2-minute session into gigabytes of self-perturbed CSV)
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --interval)    interval="$2"; shift 2 ;;
        --label)       label="$2"; shift 2 ;;
        --draw-frames) draw_frames="$2"; shift 2 ;;
        --no-arm)      no_arm=1; shift ;;
        -h|--help)     usage; exit 0 ;;
        *) echo "record: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

mode="off"; fifo=""; capture_file=""; markers=0
if [[ -r $SESSION_MODE ]]; then
    # shellcheck source=/dev/null
    source "$SESSION_MODE"
else
    echo "record: no $SESSION_MODE — did you start the game with launch-game-debug.sh?" >&2
    echo "record: continuing in survey-only mode (sampler runs, no INTEL_MEASURE)." >&2
fi

# --- find the game -------------------------------------------------------------------
game_pid=$(pgrep -f -- "$GAME_MATCH" 2>/dev/null | head -1 || true)
if [[ -z $game_pid ]]; then
    game_pid=$(pgrep -f -- "Palworld" 2>/dev/null | head -1 || true)
fi
if [[ -z $game_pid ]]; then
    echo "record: no running Palworld process found." >&2
    echo "record: start it with ./scripts/launch-game-debug.sh and get in-game first." >&2
    exit 1
fi
echo "record: game pid $game_pid ($(cat "/proc/$game_pid/comm" 2>/dev/null || echo '?'))"

# Confirm it is actually the process holding DRM fds; if not, the sampler sees nothing.
drm_fds=0
for f in /proc/"$game_pid"/fd/*; do
    tgt=$(readlink "$f" 2>/dev/null) || continue
    case "$tgt" in */dri/*) drm_fds=$((drm_fds + 1)) ;; esac
done
if [[ $drm_fds -eq 0 ]]; then
    echo "record: WARNING pid $game_pid holds no DRM fds — engine-busy and VRAM columns" >&2
    echo "record: will be empty. The rendering process is probably a child; check" >&2
    echo "record:   pgrep -af Palworld" >&2
else
    echo "record: pid holds $drm_fds DRM fd(s) — good"
fi

survey_out="$CAPTURES/survey_${STAMP}.csv"

# --- arm ------------------------------------------------------------------------------
arm_frames=""
if [[ $no_arm -eq 1 ]]; then
    echo "record: --no-arm — NOT touching the control fifo; sampler only."
    echo "record: (writing the fifo is the only thing this script does to the game, and it"
    echo "record:  is the open suspect for the vkQueueSubmit2 abort — context/pitfalls.md)"
elif [[ $mode != "off" && -z ${fifo:-} ]]; then
    echo "record: mode=$mode launched with --no-fifo — INTEL_MEASURE has been capturing"
    echo "record: since driver init. Nothing to arm; the window below is just a marker."
    echo "record: capture -> ${capture_file:-?}"
elif [[ $mode != "off" && -n ${fifo:-} && -p ${fifo:-} ]]; then
    if [[ $mode == "draw" ]]; then
        arm_frames="$draw_frames"
        echo "record: mode=draw — arming for $arm_frames frames only (hard cap; per-draw"
        echo "record: flushing perturbs the very frame times it reports)"
    else
        arm_frames="$OPEN_ENDED_FRAMES"
        echo "record: mode=$mode — arming open-ended; Ctrl-C disarms"
    fi
    if timeout 5 bash -c "echo $arm_frames > '$fifo'" 2>/dev/null; then
        echo "record: INTEL_MEASURE armed -> ${capture_file:-?}"
    else
        echo "record: WARNING could not write to $fifo within 5 s." >&2
        echo "record: That means nothing is reading it — the game was probably not" >&2
        echo "record: launched through proton-profile-wrapper.sh. Check:" >&2
        echo "record:   cat $CAPTURES/last-launch.log" >&2
        arm_frames=""
    fi
elif [[ $mode == "utrace" ]]; then
    echo "record: mode=utrace — u_trace has no control fifo; it has been recording since"
    echo "record: driver init. The window below is what you care about; trim to it."
else
    echo "record: no INTEL_MEASURE capture armed (mode=${mode})"
fi

cat <<EOF

  ── capturing ────────────────────────────────────────────────────────────────
  Optional cross-check: press Shift_L+F2 in-game now to start MangoHud logging
  (frame times; our sampler covers GPU busy, VRAM, clocks and throttle).

  Ctrl-C when done.
  ─────────────────────────────────────────────────────────────────────────────

EOF

window_start=$(date -Is)

cleanup() {
    trap - INT TERM
    echo
    echo "record: stopping ..."
    if [[ -n $arm_frames && -n ${fifo:-} && -p ${fifo:-} ]]; then
        if timeout 5 bash -c "echo 0 > '$fifo'" 2>/dev/null; then
            echo "record: INTEL_MEASURE disarmed"
        else
            echo "record: WARNING could not disarm the fifo (game already gone?)" >&2
        fi
    fi
    if [[ -n ${sampler_pid:-} ]] && kill -0 "$sampler_pid" 2>/dev/null; then
        kill -INT "$sampler_pid" 2>/dev/null || true
        wait "$sampler_pid" 2>/dev/null || true
    fi

    echo
    echo "record: window $window_start .. $(date -Is)"
    echo "record: survey CSV   $survey_out"
    if [[ -n ${capture_file:-} ]]; then
        if [[ -f $capture_file ]]; then
            echo "record: capture      $capture_file ($(wc -l <"$capture_file") lines, $(du -h "$capture_file" | cut -f1))"
        else
            echo "record: WARNING expected capture $capture_file does not exist — nothing was written." >&2
            echo "record: check $CAPTURES/last-launch.log to confirm INTEL_MEASURE reached the game." >&2
        fi
    fi
    echo
    echo "record: reminder — press Shift_L+F2 again if you started MangoHud logging."
    exit 0
}
trap cleanup INT TERM

"$REPO_DIR/scripts/gpu-survey.sh" \
    --pid "$game_pid" \
    --interval "$interval" \
    --out "$survey_out" \
    --label "${label:-mode=$mode}" &
sampler_pid=$!

wait "$sampler_pid" || true
cleanup
