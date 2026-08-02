#!/usr/bin/env bash
# power-load-run.sh — drive a sustained GPU load while sampling, N times. Plan 07 T3/T4.
#
# Why this exists: the power-limit question needs N >= 3 runs of the same load with the
# same sampling, and Rule D wants each one named and recorded. Doing that by hand invites
# a run with a different duration sneaking into the comparison.
#
# The load is `vkmark`, which on this box drives the A310 to rp0 (2450 MHz) and pins
# package power at the PL1 value within ~2 s. It is not a realistic frame workload — it
# is a power-limit exerciser, which is exactly what T3 needs. Palworld answers the
# separate question of whether the limit binds in the actual game.
#
# NB on style: follows gpu-survey.sh — no `[[ cond ]] && assign` idiom. Under `set -e`
# such a statement returns non-zero when the condition is false and silently kills the
# script.
#
# NB on stopping the sampler: use SIGTERM, never SIGINT. gpu-survey.sh traps both, but a
# background (`&`) child of a NON-INTERACTIVE shell inherits SIGINT as SIG_IGN, and bash
# cannot re-trap a signal that was ignored on entry — so `trap finish INT` in the child is
# a no-op here and `kill -INT` is silently swallowed. Observed 2026-08-01: the sampler ran
# on through three loads, producing one 919-sample CSV instead of three 150-sample ones,
# and the script hung in `wait`. stop_survey() below uses TERM with a bounded wait and a
# KILL fallback so a stuck sampler can never wedge the run again.

set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
CAPTURES="$REPO/captures"
CARD_DEV=/sys/class/drm/card0/device

duration=75
runs=3
cooldown=25
label=""
scene="shading"
interval=0.5
hud=1

# What the on-screen HUD shows. Deliberately omits gpu_power: it is a valid MangoHud key
# but xe exposes no instantaneous power sensor at all (no HWMON_P_INPUT/HWMON_P_AVERAGE in
# xe_hwmon.c's hwmon_info[]), so it would sit permanently blank — which is precisely the
# confusion that started plan 07. Watts come from the CSV's power_w column instead.
# throttling_status is the one to watch live: it is the same signal the plan turns on.
# Override wholesale by exporting MANGOHUD_CONFIG before calling.
DEFAULT_MANGOHUD_CONFIG=${MANGOHUD_CONFIG:-\
frametime,gpu_stats,gpu_temp,gpu_core_clock,gpu_mem_clock,vram,\
cpu_stats,cpu_temp,throttling_status,engine_version,vulkan_driver}

usage() {
    cat <<'EOF'
Usage: power-load-run.sh --label TXT [--duration S] [--runs N] [--cooldown S] [--scene S]

  --label TXT     required; goes in the capture filename and the CSV header
  --duration S    load duration per run in seconds (default 75 — must exceed the 28 s
                  PL1 time constant with room for a >= 30 s plateau window)
  --runs N        number of runs (default 3 — RULES.md D.1 wants >= 3)
  --cooldown S    idle seconds between runs, to avoid carrying thermal state (default 25)
  --scene S       vkmark scene (default shading)
  --interval S    sampling period (default 0.5)
  --hud           show the MangoHud overlay during the load (default: on)
  --no-hud        run without the overlay

The HUD is a Vulkan layer in the load path, so it is part of the workload. Whether it was
on is recorded in each CSV header — do not compare a hud=on run against a hud=off one
without saying so (RULES.md D.4). Watts are NOT on the overlay; xe has no power sensor for
MangoHud to read. Read power_w from the CSV.

Writes captures/survey_<label>_<date>_runN.csv and prints the analyze/power_summary.py
verdict across all runs.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --label)    label="$2"; shift 2 ;;
        --duration) duration="$2"; shift 2 ;;
        --runs)     runs="$2"; shift 2 ;;
        --cooldown) cooldown="$2"; shift 2 ;;
        --scene)    scene="$2"; shift 2 ;;
        --interval) interval="$2"; shift 2 ;;
        --hud)      hud=1; shift ;;
        --no-hud)   hud=0; shift ;;
        -h|--help)  usage; exit 0 ;;
        *) echo "power-load-run: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ -z "$label" ]]; then
    echo "power-load-run: --label is required — an unnamed capture is a dead capture" >&2
    usage >&2
    exit 2
fi

if ! command -v vkmark >/dev/null 2>&1; then
    echo "power-load-run: vkmark not found" >&2
    exit 1
fi

# Build the load command. mangohud(1) is a wrapper that sets the LD_PRELOAD shim, so it
# prefixes the real binary. If it is missing we say so and continue unhudded rather than
# failing the run — losing the overlay is cosmetic, losing the measurement is not.
load_cmd=(vkmark -b "${scene}:duration=${duration}")
hud_state="off"
if [[ $hud -eq 1 ]]; then
    if command -v mangohud >/dev/null 2>&1; then
        load_cmd=(mangohud "${load_cmd[@]}")
        hud_state="on"
        export MANGOHUD_CONFIG="$DEFAULT_MANGOHUD_CONFIG"
    else
        echo "power-load-run: --hud requested but mangohud not found; continuing without" >&2
    fi
fi

if (( duration < 60 )); then
    echo "power-load-run: --duration $duration is under 60 s. The PL1 tau is 28 s, so a" >&2
    echo "power-load-run: shorter run has no >= 30 s plateau to judge. Refusing." >&2
    exit 2
fi

# Stop the sampler and make sure it is really gone. See the SIGINT note in the header.
stop_survey() {
    local pid=$1 waited=0
    kill -TERM "$pid" 2>/dev/null || true
    while (( waited < 50 )) && kill -0 "$pid" 2>/dev/null; do
        sleep 0.1
        waited=$(( waited + 1 ))
    done
    if kill -0 "$pid" 2>/dev/null; then
        echo "power-load-run: sampler $pid ignored TERM after 5 s — sending KILL" >&2
        kill -KILL "$pid" 2>/dev/null || true
    fi
    wait "$pid" 2>/dev/null || true
}

mkdir -p "$CAPTURES"

stamp=$(date +%Y-%m-%d_%H%M%S)
# Record the limit in force, so the capture is self-describing if it is read months later.
limit_uw=$(cat "$CARD_DEV"/hwmon/hwmon*/power2_max 2>/dev/null || echo "NA")
limit_w="NA"
if [[ "$limit_uw" != "NA" ]]; then
    limit_w=$(( limit_uw / 1000000 ))
fi

echo "power-load-run: label=$label runs=$runs duration=${duration}s scene=$scene hud=$hud_state"
echo "power-load-run: power2_max in force = ${limit_uw} uW"
if [[ "$hud_state" == "on" ]]; then
    echo "power-load-run: HUD on — MANGOHUD_CONFIG=$MANGOHUD_CONFIG"
    echo "power-load-run: no watts on the overlay (xe has no power sensor); see power_w in the CSV"
fi
echo

outputs=()
for (( i = 1; i <= runs; i++ )); do
    out="$CAPTURES/survey_${label}_${stamp}_run${i}.csv"
    echo "power-load-run: run $i/$runs -> $(basename "$out")"

    "$HERE/gpu-survey.sh" --interval "$interval" \
        --label "$label run$i vkmark:$scene pl1=${limit_uw}uW hud=$hud_state" \
        --out "$out" >/dev/null 2>&1 &
    survey=$!

    sleep 1
    # vkmark writes its score to stdout; we only care that it ran for the full duration.
    if ! timeout $(( duration + 30 )) vkmark -b "${scene}:duration=${duration}" >/dev/null 2>&1; then
        echo "power-load-run: vkmark failed or timed out on run $i" >&2
        stop_survey "$survey"
        exit 1
    fi

    stop_survey "$survey"
    outputs+=("$out")

    if (( i < runs )); then
        echo "power-load-run: cooldown ${cooldown}s"
        sleep "$cooldown"
    fi
done

echo
echo "power-load-run: summarising"
echo
if [[ "$limit_w" != "NA" ]]; then
    "$REPO/analyze/power_summary.py" --limit-w "$limit_w" "${outputs[@]}"
else
    "$REPO/analyze/power_summary.py" "${outputs[@]}"
fi
