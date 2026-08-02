#!/usr/bin/env bash
# power-pl1-disable-probe.sh — is the RAPL limit register actually ours to write? Plan 07 T5.
#
# `echo 0 > power2_max` is the ONE power-limit operation the DG2 path verifies. Every other
# write returns success unconditionally (xe_hwmon.c:442 never assigns ret), but the disable
# path clears PWR_LIM_EN, re-reads, and reports failure if the bit refuses to drop:
#
#     reg_val = xe_mmio_rmw32(mmio, rapl_limit, PWR_LIM_EN, 0);
#     reg_val = xe_mmio_read32(mmio, rapl_limit);
#     if (reg_val & PWR_LIM_EN) {
#             drm_warn(&hwmon->xe->drm, "Power limit disable is not supported!\n");
#             ret = -EOPNOTSUPP;
#     }
#                                                      -- xe_hwmon.c:385-402
#
# So this is the definitive test of whether the PCU lets us touch 0x1459A0 at all. If it
# returns -EOPNOTSUPP, no kernel patch would change that and the power thread is closed.
#
# SAFETY
#   - PL1 is restored on EVERY exit path, including Ctrl-C and errors (trap restore EXIT).
#   - The GPU is left IDLE throughout. This probe does not run a load: the question is
#     whether the register accepts the write, not what happens at unlimited power on a
#     75 W-slot card. Do not add a load here.
#   - PL2, PL4, thermal and VR limits all remain in force regardless.
#   - Temperatures are sampled before and after and printed.
#
# THE ONE FOOTGUN: while PWR_LIM_EN is clear, xe_hwmon_power_is_visible() returns 0 for
# power2_max (xe_hwmon.c:1085-1092), so a driver reload or reboot in that window would make
# power2_max and power2_max_interval disappear from sysfs entirely until the next boot with
# the bit set. The restore below closes the window in well under a second. Do not walk away
# mid-probe.
#
# NB on style: follows gpu-survey.sh — no `[[ cond ]] && assign` idiom under `set -e`.

set -euo pipefail

HWMON_DIR=$(dirname "$(echo /sys/class/drm/card0/device/hwmon/hwmon*/power2_max)")
PL1="$HWMON_DIR/power2_max"

if [[ ! -e "$PL1" ]]; then
    echo "power-pl1-disable-probe: $PL1 not found." >&2
    echo "power-pl1-disable-probe: if PL1 was left disabled by an earlier run, this file is" >&2
    echo "power-pl1-disable-probe: gone until a reboot — that is the documented footgun." >&2
    exit 1
fi

if [[ $EUID -eq 0 ]]; then
    echo "power-pl1-disable-probe: run as yourself; it sudo's for the two writes." >&2
    exit 2
fi

read_temps() {
    local f label val
    for f in "$HWMON_DIR"/temp*_input; do
        if [[ -e "$f" ]]; then
            label=$(cat "${f%_input}_label" 2>/dev/null || echo "${f##*/}")
            val=$(cat "$f")
            printf '   %-6s %s °C\n' "$label" "$(( val / 1000 ))"
        fi
    done
}

original_uw=$(cat "$PL1")
restored=0

restore() {
    if [[ $restored -eq 1 ]]; then
        return
    fi
    restored=1
    echo
    echo "── restoring PL1 to ${original_uw} uW"
    if ! echo "$original_uw" | sudo tee "$PL1" >/dev/null 2>&1; then
        echo "   !! RESTORE FAILED. Run this by hand NOW:" >&2
        echo "      echo $original_uw | sudo tee $PL1" >&2
        echo "   If power2_max has already vanished from sysfs, reboot to get it back." >&2
        return
    fi
    local now
    now=$(cat "$PL1" 2>/dev/null || echo "GONE")
    if [[ "$now" == "$original_uw" ]]; then
        echo "   restored, power2_max = $now"
    else
        echo "   !! readback is '$now', expected $original_uw" >&2
    fi
}
trap restore EXIT INT TERM

echo "power-pl1-disable-probe: original PL1 = ${original_uw} uW"
echo
echo "── temperatures before"
read_temps
echo "   throttle: $(cat /sys/class/drm/card0/device/tile0/gt0/freq0/throttle/reasons)"
echo

# Remember how long the kernel log is, so anything printed afterwards can be attributed to
# this probe rather than to history. Deliberately does NOT touch the console loglevel —
# `dmesg -n` is a persistent system change and this script leaves no trace but the restore.
dmesg_before=$(sudo dmesg 2>/dev/null | wc -l)

echo "── writing 0 to power2_max (disable PL1)"
set +e
disable_output=$(echo 0 | sudo tee "$PL1" 2>&1 >/dev/null)
disable_rc=$?
set -e
echo "   exit status: $disable_rc"
if [[ -n "$disable_output" ]]; then
    echo "   stderr: $disable_output"
fi

readback=$(cat "$PL1" 2>/dev/null || echo "GONE")
echo "   readback: $readback"
echo

echo "── new kernel messages since the write"
sudo dmesg 2>/dev/null | tail -n +$(( dmesg_before + 1 )) | sed 's/^/   /' || true
echo

echo "── temperatures after"
read_temps
echo

echo "══ Verdict"
if [[ $disable_rc -ne 0 ]]; then
    echo "   REFUTED: the write failed (exit $disable_rc). If the kernel log above shows"
    echo "   'Power limit disable is not supported!' then the PCU refused to clear"
    echo "   PWR_LIM_EN and returned -EOPNOTSUPP. The RAPL limit register is effectively"
    echo "   read-only to us, and NO kernel patch changes that — the power thread is closed."
elif [[ "$readback" == "0" ]]; then
    echo "   CONFIRMED: PL1 is genuinely disableable — the register is under our control."
    echo "   The remaining ceiling is PL2 / PL4 / thermal / VR, none of which xe exposes"
    echo "   on DG2. Combined with T4, this tells you whether raising PL1 is worth pursuing."
else
    echo "   AMBIGUOUS: the write reported success (exit 0) but readback is '$readback',"
    echo "   not 0. Record this verbatim in the tracker rather than interpreting it."
fi

# restore() runs from the EXIT trap.
