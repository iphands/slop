#!/usr/bin/env bash
# power-pl1-experiment.sh — raise PL1, measure, restore. Plan 07 T4.
#
# T3 established that this card is power-limited: package power plateaus at 31.20 W
# against a 31.25 W PL1, run-to-run spread 0.00 W over 3 runs, with cur_freq pinned at
# rp0 the whole time. This asks the follow-up: does the PCU actually honour a HIGHER PL1,
# or does it silently clamp?
#
# The driver cannot answer that. On DG2 xe_hwmon_power_max_write() takes the plain-MMIO
# branch (xe_pci.c:349, .has_mbx_power_limits = false), never assigns ret, and so reports
# success unconditionally (xe_hwmon.c:442). Its read path re-reads the same register with
# the PKG_POWER_SKU clamp inert, because T2 found that register reads 0. A successful
# readback therefore proves only that the MMIO word holds the value. Only measured power
# under load can distinguish "honoured" from "silently clamped".
#
# Also settles the alias question T2 raised: 0x1459a4 read byte-identical to 0x1459a0. If
# writing PL1 moves BOTH, the address aliases and PL2 is not reachable there.
#
# SAFETY
#   - PL1 is restored on ANY exit path, including Ctrl-C and errors (trap restore EXIT).
#   - The requested limit is capped by --max-w (default 50 W) so a typo cannot ask for
#     something absurd. PL2, PL4, thermal and VR limits all remain in force throughout;
#     this only moves the sustained-power ceiling.
#   - Needs sudo only for the sysfs write and the register read. The GPU load runs as you,
#     because vkmark needs your Wayland session.
#
# NB on style: follows gpu-survey.sh — no `[[ cond ]] && assign` idiom under `set -e`.

set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/.." && pwd)

HWMON=$(echo /sys/class/drm/card0/device/hwmon/hwmon*/power2_max)

target_w=50
max_w=50
duration=75
runs=3
cooldown=25
label="t4-raised"
hud_args=()

usage() {
    cat <<'EOF'
Usage: power-pl1-experiment.sh [--target-w N] [--duration S] [--runs N] [--label TXT]

  --target-w N   PL1 to test, in watts (default 50)
  --max-w N      refuse a target above this, in watts (default 50) — typo guard
  --duration S   load duration per run (default 75)
  --runs N       runs at the raised limit (default 3)
  --cooldown S   idle seconds between runs (default 25)
  --label TXT    capture label (default t4-raised)
  --no-hud       run the load without the MangoHud overlay. Use this to stay comparable
                 with the hud=off t3-baseline and t4-raised captures from 2026-08-01/02.

Restores the original PL1 on every exit path. Run as your normal user; it calls sudo
itself for the two privileged steps.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target-w) target_w="$2"; shift 2 ;;
        --max-w)    max_w="$2"; shift 2 ;;
        --duration) duration="$2"; shift 2 ;;
        --runs)     runs="$2"; shift 2 ;;
        --cooldown) cooldown="$2"; shift 2 ;;
        --no-hud)   hud_args+=(--no-hud); shift ;;
        --hud)      hud_args+=(--hud); shift ;;
        --label)    label="$2"; shift 2 ;;
        -h|--help)  usage; exit 0 ;;
        *) echo "power-pl1-experiment: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ ! -w "$HWMON" ]] && [[ ! -e "$HWMON" ]]; then
    echo "power-pl1-experiment: $HWMON not found" >&2
    exit 1
fi

if (( target_w > max_w )); then
    echo "power-pl1-experiment: --target-w $target_w exceeds --max-w $max_w. Refusing." >&2
    echo "power-pl1-experiment: raise --max-w deliberately if you really mean it." >&2
    exit 2
fi

if [[ $EUID -eq 0 ]]; then
    echo "power-pl1-experiment: do NOT run this whole script as root — vkmark needs your" >&2
    echo "power-pl1-experiment: Wayland session. Run it as yourself; it sudo's internally." >&2
    exit 2
fi

original_uw=$(cat "$HWMON")
target_uw=$(( target_w * 1000000 ))
restored=0

restore() {
    if [[ $restored -eq 1 ]]; then
        return
    fi
    restored=1
    echo
    echo "power-pl1-experiment: restoring PL1 to ${original_uw} uW"
    if ! echo "$original_uw" | sudo tee "$HWMON" >/dev/null; then
        echo "power-pl1-experiment: !! RESTORE FAILED. Set it by hand:" >&2
        echo "    echo $original_uw | sudo tee $HWMON" >&2
        return
    fi
    local now
    now=$(cat "$HWMON")
    if [[ "$now" == "$original_uw" ]]; then
        echo "power-pl1-experiment: restored, power2_max = $now"
    else
        echo "power-pl1-experiment: !! readback is $now, expected $original_uw" >&2
    fi
}
trap restore EXIT INT TERM

echo "power-pl1-experiment: original PL1 = ${original_uw} uW"
echo "power-pl1-experiment: target   PL1 = ${target_uw} uW (${target_w} W)"
echo

echo "── registers BEFORE the write"
sudo "$HERE/power-regs.sh" --raw
echo

echo "── writing PL1"
echo "$target_uw" | sudo tee "$HWMON" >/dev/null
readback=$(cat "$HWMON")
echo "   sysfs readback: $readback"
if [[ "$readback" != "$target_uw" ]]; then
    echo "   NOTE: readback differs from what was written — the driver or PCU adjusted it."
fi
echo

echo "── registers AFTER the write  (does 0x1459a4 move with 0x1459a0? -> alias)"
sudo "$HERE/power-regs.sh" --raw
echo

echo "── load runs at the raised limit"
"$HERE/power-load-run.sh" --label "$label" --duration "$duration" --runs "$runs" \
    --cooldown "$cooldown" "${hud_args[@]}"

echo
echo "── registers AFTER the load  (did the punit revert PL1 on its own?)"
sudo "$HERE/power-regs.sh" --raw

# restore() runs from the EXIT trap.
