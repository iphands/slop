#!/usr/bin/env bash
# power-pl2-experiment.sh — raise PL2 (and PL1) past stock and measure. Plan 07 T4b.
#
# THIS IS THE UNSUPPORTED PATH. Everything else in plan 07 goes through sysfs and the
# driver. This writes a register the kernel never touches, with intel_reg, behind the
# driver's back.
#
# Why it exists: T4 proved PL2 at 0x1459A4 is the binding limit and that xe provides no way
# to reach it. PL1 (power2_max) writes land and persist and do nothing — the plateau stayed
# at 31.20 W with PL1 at 50 W. 0x1459A4 is the only known lever left.
#
# WHY IT RAISES *BOTH* LIMITS. Writing PL2 alone accomplishes nothing: PL1 is 31.25 W, so it
# would simply become the lower of the two and bind instead, and the plateau would not move.
# The PCU enforces whichever is lower. So this sets PL1 (via sysfs, the supported path) and
# PL2 (via intel_reg) to the same target.
#
# ── READ THIS BEFORE RUNNING ────────────────────────────────────────────────────────────
#
# * SLOT POWER. The pkg domain is capped at 31.25 W stock. Total BOARD draw is higher —
#   pkg plus VRAM, VR losses and fans — and we have no way to measure it: xe exposes only
#   the pkg energy counter. A310 boards are commonly 75 W slot-powered with NO PCIe power
#   connector. If yours has none, the board budget above stock is small and you can ask the
#   slot for more than the 75 W it is specified to deliver. **Physically check the card for
#   an external power connector before running this.** --confirm exists to make that a
#   deliberate act.
#
# * The register is restored on every exit path (trap), and a temperature watchdog aborts
#   the run if the package exceeds --temp-limit. Neither helps if the machine hard-hangs.
#
# * A reboot is the ultimate reset: 0x1459A4 is programmed at init and read 0x00dc80fa on
#   two separate boots (2026-08-01, 2026-08-02). Nothing here survives a power cycle.
#
# * intel_reg writes race the driver. Registers are only written while the GPU is IDLE —
#   before the load starts and after it ends. The watchdog kills the load first, then
#   restores, for the same reason.
#
# NB on style: follows gpu-survey.sh — no `[[ cond ]] && assign` idiom under `set -e`.

set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

PL2_REG=0x1459a4
PL1_REG=0x1459a0
HWMON_DIR=$(dirname "$(echo /sys/class/drm/card0/device/hwmon/hwmon*/power2_max)")
PL1_SYSFS="$HWMON_DIR/power2_max"
TEMP_INPUT="$HWMON_DIR/temp2_input"

# Stock values, observed on two boots. Used as the restore target and sanity-checked below.
STOCK_PL2=0x00dc80fa
# Bits 23:17 of the RAPL registers are the time window (x=3, y=14 -> 28 s). Preserved.
TAU_BITS=0x00dc0000
EN_BIT=0x8000
# Power fields are U12.3 — 1/8 W per count (PKG_POWER_SKU_UNIT = 0x000a0e03).
COUNTS_PER_W=8

target_w=35
max_w=50
temp_limit=90
duration=75
runs=3
cooldown=25
label="t4b-pl2-raised"
confirmed=0
hud_args=()

usage() {
    cat <<'EOF'
Usage: power-pl2-experiment.sh --confirm [--target-w N] [--max-w N] [--temp-limit C]

  --confirm       REQUIRED. Acknowledges that you have physically checked whether the card
                  has a PCIe power connector, and accept writing a register the driver does
                  not manage. See the slot-power note in the script header.
  --target-w N    PL1 and PL2 target in watts (default 35 — a deliberately small first step
                  over the 31.25 W stock; +3.75 W is ~30x the 0.125 W noise floor, so it is
                  unambiguously detectable while staying close to the stock board budget)
  --max-w N       refuse a target above this (default 50)
  --temp-limit C  watchdog aborts and restores above this package temperature (default 90)
  --duration S    load seconds per run (default 75)
  --runs N        runs (default 3)
  --cooldown S    idle seconds between runs (default 25)
  --no-hud        run the load without the MangoHud overlay
  --label TXT     capture label (default t4b-pl2-raised)

Escalate deliberately: run at 35 first and read the result before trying 40 or 45. If the
plateau does not move at 35, PL2 is not the lever either and there is nothing further to try
from software.

Run as your normal user; it calls sudo itself. Restores PL1 and PL2 on every exit path.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --confirm)    confirmed=1; shift ;;
        --target-w)   target_w="$2"; shift 2 ;;
        --max-w)      max_w="$2"; shift 2 ;;
        --temp-limit) temp_limit="$2"; shift 2 ;;
        --duration)   duration="$2"; shift 2 ;;
        --runs)       runs="$2"; shift 2 ;;
        --cooldown)   cooldown="$2"; shift 2 ;;
        --label)      label="$2"; shift 2 ;;
        --no-hud)     hud_args+=(--no-hud); shift ;;
        --hud)        hud_args+=(--hud); shift ;;
        -h|--help)    usage; exit 0 ;;
        *) echo "power-pl2-experiment: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ $EUID -eq 0 ]]; then
    echo "power-pl2-experiment: run as yourself — vkmark needs your Wayland session." >&2
    exit 2
fi

if [[ $confirmed -ne 1 ]]; then
    cat >&2 <<'WARN'
power-pl2-experiment: --confirm is required.

This writes 0x1459A4 (PL2) with intel_reg, bypassing the driver, and raises the package
power ceiling above the value the board shipped with.

Before you run it, physically check the card for a PCIe power connector. The A310 pkg
domain is capped at 31.25 W stock; total board draw is higher and we cannot measure it.
If the board is slot-powered only, its budget is 75 W and the headroom above stock is
small.

If you have checked, re-run with --confirm.
WARN
    exit 2
fi

if (( target_w > max_w )); then
    echo "power-pl2-experiment: --target-w $target_w exceeds --max-w $max_w. Refusing." >&2
    exit 2
fi

for tool in intel_reg vkmark; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "power-pl2-experiment: $tool not found" >&2
        exit 1
    fi
done

# Pin the target explicitly. intel_reg defaults to "first matched Intel GPU", which is the
# wrong card the moment an iGPU is also present — and writing a power register on the wrong
# device is not a mistake worth risking for the sake of one flag.
PCI_SLOT=$(basename "$(readlink -f /sys/class/drm/card0/device)")

read_reg() {
    sudo intel_reg --pci-slot="$PCI_SLOT" read "mmio:$1" 2>/dev/null |
        sed -nE 's/.*\):[[:space:]]*(0x[0-9a-fA-F]+).*/\1/p' | tail -1
}

write_reg() {
    sudo intel_reg --pci-slot="$PCI_SLOT" write "mmio:$1" "$2" >/dev/null
}

pkg_temp_c() {
    local v
    v=$(cat "$TEMP_INPUT" 2>/dev/null || echo 0)
    echo $(( v / 1000 ))
}

original_pl1_uw=$(cat "$PL1_SYSFS")
original_pl2=$(read_reg "$PL2_REG")

if [[ -z "$original_pl2" ]]; then
    echo "power-pl2-experiment: could not read $PL2_REG — is sudo working?" >&2
    exit 1
fi

# Refuse to proceed from an unexpected starting point. If PL2 is not at the value both
# prior boots showed, something already changed it and blindly "restoring" STOCK_PL2 later
# could make things worse rather than better.
if [[ $(( original_pl2 )) -ne $(( STOCK_PL2 )) ]]; then
    echo "power-pl2-experiment: PL2 reads $original_pl2, expected the stock $STOCK_PL2." >&2
    echo "power-pl2-experiment: something already modified it. Reboot to get a known state," >&2
    echo "power-pl2-experiment: or investigate before running this. Refusing." >&2
    exit 1
fi

target_counts=$(( target_w * COUNTS_PER_W ))
target_reg=$(printf '0x%08x' $(( TAU_BITS | EN_BIT | target_counts )))
target_uw=$(( target_w * 1000000 ))

restored=0
watchdog_pid=""

restore() {
    if [[ $restored -eq 1 ]]; then
        return
    fi
    restored=1

    if [[ -n "$watchdog_pid" ]]; then
        kill -TERM "$watchdog_pid" 2>/dev/null || true
        wait "$watchdog_pid" 2>/dev/null || true
    fi

    echo
    echo "── restoring"
    # Order matters: stop any load first so the register write does not race the GPU.
    pkill -TERM -x vkmark 2>/dev/null || true
    sleep 1

    write_reg "$PL2_REG" "$original_pl2" || true
    echo "$original_pl1_uw" | sudo tee "$PL1_SYSFS" >/dev/null 2>&1 || true

    local now_pl2 now_pl1
    now_pl2=$(read_reg "$PL2_REG")
    now_pl1=$(cat "$PL1_SYSFS" 2>/dev/null || echo GONE)
    echo "   PL2 $PL2_REG = $now_pl2 (was $original_pl2)"
    echo "   PL1 sysfs    = $now_pl1 (was $original_pl1_uw)"

    if [[ "$now_pl2" != "$original_pl2" ]] || [[ "$now_pl1" != "$original_pl1_uw" ]]; then
        echo "   !! RESTORE INCOMPLETE. Fix by hand:" >&2
        echo "      sudo intel_reg write mmio:$PL2_REG $original_pl2" >&2
        echo "      echo $original_pl1_uw | sudo tee $PL1_SYSFS" >&2
        echo "   A reboot also resets both." >&2
    fi
}
trap restore EXIT INT TERM

echo "power-pl2-experiment: target ${target_w} W"
echo "   PL1 sysfs  ${original_pl1_uw} uW -> ${target_uw} uW"
echo "   PL2 $PL2_REG $original_pl2 -> $target_reg"
echo "   temp watchdog: abort above ${temp_limit} °C (now $(pkg_temp_c) °C)"
echo

echo "── writing both limits (GPU idle)"
echo "$target_uw" | sudo tee "$PL1_SYSFS" >/dev/null
write_reg "$PL2_REG" "$target_reg"

after_pl1=$(read_reg "$PL1_REG")
after_pl2=$(read_reg "$PL2_REG")
echo "   PL1 $PL1_REG = $after_pl1"
echo "   PL2 $PL2_REG = $after_pl2"

if [[ $(( after_pl2 )) -ne $(( target_reg )) ]]; then
    echo
    echo "   PL2 did not take the write — reads $after_pl2, wanted $target_reg."
    echo "   The register is write-protected. That is a complete answer: there is no"
    echo "   software path to this card's power ceiling. Stopping before the load."
    exit 0
fi
echo

# Watchdog: poll temperature, and on breach kill the load so the EXIT trap can restore
# against an idle GPU. Runs in the background for the duration of the loads.
(
    while true; do
        t=$(cat "$TEMP_INPUT" 2>/dev/null || echo 0)
        if (( t / 1000 >= temp_limit )); then
            echo "power-pl2-experiment: WATCHDOG — package hit $(( t / 1000 )) °C, killing load" >&2
            pkill -TERM -x vkmark 2>/dev/null || true
            exit 1
        fi
        sleep 1
    done
) &
watchdog_pid=$!

echo "── load runs at ${target_w} W"
"$HERE/power-load-run.sh" --label "$label" --duration "$duration" --runs "$runs" \
    --cooldown "$cooldown" "${hud_args[@]}"

echo
echo "── registers after the load"
echo "   PL1 $PL1_REG = $(read_reg "$PL1_REG")"
echo "   PL2 $PL2_REG = $(read_reg "$PL2_REG")"
echo "   package temp = $(pkg_temp_c) °C"
echo
echo "══ Compare against the stock baseline (both hud=off, same load):"
echo "   PL1 31.25 / PL2 31.25 -> 31.20 W, spread 0.00 W, 3 runs"
echo "   PL1 50.00 / PL2 31.25 -> 31.20 W, spread 0.00 W, 3 runs"
echo "   A plateau still at 31.20 W means PL2 is not the lever either, and nothing in"
echo "   software moves this card's ceiling. Anything above means it is, and by how much."

# restore() runs from the EXIT trap.
