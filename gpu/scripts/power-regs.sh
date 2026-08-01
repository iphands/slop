#!/usr/bin/env bash
# power-regs.sh — dump and decode the DG2/A310 power & frequency registers from MMIO.
#
# Why this exists: Plan 07 T2. sysfs shows what the `xe` driver chooses to expose, which on
# DG2 is a strict subset — `power2_rated_max` is hidden whenever PCU_CR_PACKAGE_POWER_SKU
# reads 0, PL2 is never plumbed at all, and the *sticky* half of the throttle-reason
# register is masked off. This reads the registers directly so we can see what the driver
# is looking at, not what it decided to tell us.
#
# Reading the registers needs root: intel_reg maps BAR0 via /sys/bus/pci/.../resource0.
# `--self-test` exercises the decoding without root or hardware.
#
# SAFETY: intel_reg maps BAR0 directly, racing the driver. This script only ever READS.
# Do not run `intel_reg write` against these addresses while a workload is running.
#
# Register and field definitions come from the kernel source this box runs
# (7.1.5-201.fc44, stock upstream — no Fedora patch touches drivers/gpu/drm/xe):
#   drivers/gpu/drm/xe/regs/xe_mchbar_regs.h:21-46   power SKU / RAPL limit
#   drivers/gpu/drm/xe/xe_guc_pc.c:42-55             RP_STATE_CAP, FREQ_INFO_REC
#   drivers/gpu/drm/xe/regs/xe_gt_regs.h:632-654     GT0_PERF_LIMIT_REASONS
#   drivers/gpu/drm/xe/xe_hwmon.c:340-445            the read/write paths we are auditing
#
# NB on style: follows gpu-survey.sh — no `[[ cond ]] && assign` idiom. Under `set -e` such
# a statement returns non-zero when the condition is false and silently kills the script.
# Explicit `if` blocks only.

set -euo pipefail

CARD=${CARD:-card0}
CARD_DEV="/sys/class/drm/$CARD/device"

# --- Register addresses (MCHBAR mirror base 0x140000 unless noted) -----------------------
declare -A REG=(
    [PKG_POWER_SKU_LO]=0x145930      # PKG_TDP [14:0], PKG_MIN_PWR [30:16]
    [PKG_POWER_SKU_HI]=0x145934      # PKG_MAX_PWR [46:32], PKG_MAX_WIN [54:48]
    [PKG_POWER_SKU_UNIT]=0x145938    # PWR [3:0], ENERGY [12:8], TIME [19:16]
    [PKG_RAPL_LIMIT]=0x1459a0        # PL1: VAL [14:0], EN [15], TIME_Y [21:17], TIME_X [23:22]
    [PKG_RAPL_LIMIT_HI]=0x1459a4     # PL2: bits 47:32 of the 64-bit register — xe defines nothing here
    [RP_STATE_CAP]=0x145998          # RP0 [7:0], RP1 [15:8], RPN [23:16], x50 MHz
    [FREQ_INFO_REC]=0x145ef0         # RPE [15:8] x50; RPa — see the mask check below
    [GT_PERF_LIMIT_REASONS]=0x1381a8 # live reasons in 0xde3; upper half unread by xe
)
# Printed in this order.
REG_ORDER=(PKG_POWER_SKU_LO PKG_POWER_SKU_HI PKG_POWER_SKU_UNIT
           PKG_RAPL_LIMIT PKG_RAPL_LIMIT_HI RP_STATE_CAP FREQ_INFO_REC
           GT_PERF_LIMIT_REASONS)

raw_only=0
self_test=0
pci_slot=""

usage() {
    cat <<'EOF'
Usage: power-regs.sh [--pci-slot BDF] [--raw] [--self-test]

  --pci-slot BDF  target this PCI slot (default: resolved from /sys/class/drm/$CARD)
  --raw           print raw register values only, skip decoding
  --self-test     decode a synthetic fixture and assert the results; no root, no hardware

Environment:
  CARD            DRM card name to resolve the PCI slot from (default: card0)

Reading real registers requires root — intel_reg maps BAR0. Reads only; never writes.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --pci-slot) pci_slot="$2"; shift 2 ;;
        --raw)      raw_only=1; shift ;;
        --self-test) self_test=1; shift ;;
        -h|--help)  usage; exit 0 ;;
        *) echo "power-regs: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

declare -A VAL

# --- Acquire register values ---------------------------------------------------------------

read_from_hardware() {
    if [[ $EUID -ne 0 ]]; then
        echo "power-regs: needs root — intel_reg maps BAR0 via /sys/bus/pci/.../resource0." >&2
        echo "power-regs: re-run with sudo, or use --self-test to check the decoding." >&2
        exit 1
    fi

    if ! command -v intel_reg >/dev/null 2>&1; then
        echo "power-regs: intel_reg not found — install igt-gpu-tools." >&2
        exit 1
    fi

    if [[ ! -d "$CARD_DEV" ]]; then
        echo "power-regs: $CARD_DEV does not exist. Set CARD= to the right DRM card." >&2
        exit 1
    fi

    if [[ -z "$pci_slot" ]]; then
        pci_slot=$(basename "$(readlink -f "$CARD_DEV")")
    fi

    driver=$(basename "$(readlink -f "$CARD_DEV/driver")" 2>/dev/null || echo "none")
    device_id=$(cat "$CARD_DEV/device")
    vendor_id=$(cat "$CARD_DEV/vendor")

    # These addresses are DG2-specific. On BMG the driver uses PCODE mailboxes instead
    # (xe_pci.c:405, .has_mbx_power_limits = true) and PCU_CR_* means nothing here.
    # Refusing beats printing a confidently decoded table of garbage.
    if [[ "$vendor_id" != "0x8086" ]] || ! is_dg2 "$device_id"; then
        echo "power-regs: $pci_slot is vendor=$vendor_id device=$device_id, which is not DG2." >&2
        echo "power-regs: these register addresses are DG2-only — refusing to decode." >&2
        exit 1
    fi

    # intel_reg prints one line per register as "%35s (0x%08x): 0x%08x". Read them all in
    # one invocation so BAR0 is mapped once, then index the results by address.
    local args=() name
    for name in "${REG_ORDER[@]}"; do
        args+=("mmio:${REG[$name]}")
    done

    reg_output=$(intel_reg --pci-slot="$pci_slot" read "${args[@]}")

    local addr value
    while read -r addr value; do
        VAL["0x$addr"]=$value
    done < <(printf '%s\n' "$reg_output" |
             sed -nE 's/.*\(0x0*([0-9a-fA-F]+)\):[[:space:]]*0x([0-9a-fA-F]+).*/\1 0x\2/p' |
             tr 'A-F' 'a-f')
}

is_dg2() {
    local id=$((16#${1#0x}))
    # DG2 PCI ID ranges, from the DG2_* id lists in drivers/gpu/drm/xe/xe_pci.c.
    if (( (id >= 0x4f80 && id <= 0x4f88) || (id >= 0x5690 && id <= 0x5698) ||
          (id >= 0x56a0 && id <= 0x56af) || (id >= 0x56b0 && id <= 0x56b3) ||
          (id >= 0x56ba && id <= 0x56bf) || (id >= 0x56c0 && id <= 0x56c2) )); then
        return 0
    fi
    return 1
}

# Synthetic fixture: the register contents implied by this box's sysfs on 2026-08-01.
#   power2_max          31250000 uW -> PWR_LIM_VAL 250, PWR_LIM_EN set     -> 0x80fa
#   power2_max_interval 28000 ms    -> PWR_LIM_TIME x=3 y=14               -> 0xdc0000
#   rp0/rpn 2450/300 MHz            -> RP0 49, RPN 6
#   rpe 900 MHz, rpa_freq 627200    -> RPE 18, and 627200/50 = 0x3100
# These are DERIVED, not observed. --self-test proves the decoder inverts them correctly;
# it says nothing about what the hardware actually holds.
load_self_test_fixture() {
    VAL[0x145930]=0x00000000
    VAL[0x145934]=0x00000000
    VAL[0x145938]=0x000a0e03
    VAL[0x1459a0]=0x00dc80fa
    VAL[0x1459a4]=0x00000000
    VAL[0x145998]=0x00063131   # RP_STATE_CAP: RPN 6 [23:16], RP1 0x31 [15:8], RP0 0x31 [7:0]
    VAL[0x145ef0]=0x31001200   # RPa ratio 0x31 at [31:24], RPe 0x12 at [15:8]
    VAL[0x1381a8]=0x00000000
    pci_slot="self-test"
    driver="self-test"
    device_id="0x56a6"
}

get() {
    local key="${REG[$1]}"
    if [[ -z "${VAL[$key]:-}" ]]; then
        echo "power-regs: no value for $1 ($key). Raw intel_reg output:" >&2
        printf '%s\n' "${reg_output:-<none>}" >&2
        exit 1
    fi
    printf '%s' "${VAL[$key]}"
}

if [[ $self_test -eq 1 ]]; then
    load_self_test_fixture
else
    read_from_hardware
fi

# --- Decode helpers --------------------------------------------------------------------------

# field <value> <hi> <lo>
field() {
    local v=$(( $1 )) hi=$2 lo=$3
    echo $(( (v >> lo) & ((1 << (hi - lo + 1)) - 1) ))
}

sku_unit=$(get PKG_POWER_SKU_UNIT)
pwr_unit=$(field "$sku_unit" 3 0)
energy_unit=$(field "$sku_unit" 12 8)
time_unit=$(field "$sku_unit" 19 16)

# Power fields are U12.<pwr_unit> — the driver's scl_shift_power (xe_hwmon.c:1505).
pwr_div=$(( 1 << pwr_unit ))

# watts_mw <raw_field> -> milliwatts
watts_mw() {
    echo $(( ($1 * 1000) / pwr_div ))
}

fmt_w() {
    local mw=$1
    printf '%d.%03d W' $(( mw / 1000 )) $(( mw % 1000 ))
}

sku_lo=$(get PKG_POWER_SKU_LO)
sku_hi=$(get PKG_POWER_SKU_HI)
pkg_tdp=$(field "$sku_lo" 14 0)
pkg_min=$(field "$sku_lo" 30 16)
pkg_max=$(field "$sku_hi" 14 0)      # PKG_MAX_PWR is bits 46:32 of the 64-bit reg
max_win=$(field "$sku_hi" 22 16)     # PKG_MAX_WIN is bits 54:48

rapl=$(get PKG_RAPL_LIMIT)
pl1_val=$(field "$rapl" 14 0)
pl1_en=$(field "$rapl" 15 15)
pl1_pwr_lim=$(field "$rapl" 15 0)    # PWR_LIM — the 16 bits xe's RMW actually writes
tw_y=$(field "$rapl" 21 17)
tw_x=$(field "$rapl" 23 22)
# tau = (1 + x/4) * 2^y, in 1/2^time_unit s. Kernel form (xe_hwmon.c:570-578):
#   tau4 = ((1 << 2) | x) << y ; out_ms = tau4 * 1000 >> (time_unit + 2)
tau4=$(( ((1 << 2) | tw_x) << tw_y ))
tau_ms=$(( (tau4 * 1000) >> (time_unit + 2) ))

rapl_hi=$(get PKG_RAPL_LIMIT_HI)
pl2_val=$(field "$rapl_hi" 14 0)
pl2_en=$(field "$rapl_hi" 15 15)

rp_cap=$(get RP_STATE_CAP)
rp0=$(( $(field "$rp_cap" 7 0) * 50 ))
rp1=$(( $(field "$rp_cap" 15 8) * 50 ))
rpn=$(( $(field "$rp_cap" 23 16) * 50 ))

freq_info=$(get FREQ_INFO_REC)
rpe=$(( $(field "$freq_info" 15 8) * 50 ))
rpa_driver=$(( $(field "$freq_info" 31 16) * 50 ))   # what xe's RPA_MASK computes today
rpa_bits_23_16=$(field "$freq_info" 23 16)
rpa_bits_31_24=$(field "$freq_info" 31 24)
rpa_proposed=$(( rpa_bits_31_24 * 50 ))

reasons=$(get GT_PERF_LIMIT_REASONS)
live=$(( reasons & 0xde3 ))
upper=$(field "$reasons" 31 16)

decode_reasons() {
    local v=$1 out=""
    if (( v & (1 << 0)  )); then out+="prochot "; fi
    if (( v & (1 << 1)  )); then out+="thermal "; fi
    if (( v & (1 << 5)  )); then out+="ratl "; fi
    if (( v & (1 << 6)  )); then out+="vr_thermalert "; fi
    if (( v & (1 << 7)  )); then out+="vr_tdc "; fi
    if (( v & (1 << 8)  )); then out+="pl4 "; fi
    if (( v & (1 << 10) )); then out+="pl1 "; fi
    if (( v & (1 << 11) )); then out+="pl2 "; fi
    if [[ -z "$out" ]]; then out="none"; fi
    printf '%s' "$out"
}

# --- Self-test ----------------------------------------------------------------------------

if [[ $self_test -eq 1 ]]; then
    fails=0
    expect() {
        local label=$1 got=$2 want=$3
        if [[ "$got" == "$want" ]]; then
            printf '  ok   %-34s = %s\n' "$label" "$got"
        else
            printf '  FAIL %-34s = %s (expected %s)\n' "$label" "$got" "$want"
            fails=$(( fails + 1 ))
        fi
    }
    echo "== power-regs.sh --self-test =="
    echo "Fixture: register contents derived from this box's sysfs on 2026-08-01."
    echo
    expect "pwr_unit"                "$pwr_unit"     "3"
    expect "energy_unit"             "$energy_unit"  "14"
    expect "time_unit"               "$time_unit"    "10"
    expect "PWR_LIM_VAL"             "$pl1_val"      "250"
    expect "PWR_LIM_EN"              "$pl1_en"       "1"
    expect "PWR_LIM (bits 15:0)"     "$(printf '0x%04x' "$pl1_pwr_lim")" "0x80fa"
    expect "PL1 watts"               "$(fmt_w "$(watts_mw "$pl1_val")")" "31.250 W"
    expect "PWR_LIM_TIME x"          "$tw_x"         "3"
    expect "PWR_LIM_TIME y"          "$tw_y"         "14"
    expect "tau (ms)"                "$tau_ms"       "28000"
    expect "RP0"                     "$rp0"          "2450"
    expect "RPn"                     "$rpn"          "300"
    expect "RPe"                     "$rpe"          "900"
    expect "RPa as xe reports it"    "$rpa_driver"   "627200"
    expect "RPa with an 8-bit mask"  "$rpa_proposed" "2450"
    expect "throttle decode (0)"     "$(decode_reasons 0)"      "none"
    expect "throttle decode (pl1)"   "$(decode_reasons $((1<<10)))" "pl1 "
    echo
    if [[ $fails -eq 0 ]]; then
        echo "self-test: all assertions passed."
        echo "NOTE: this proves the decoder inverts the fixture correctly. It says nothing"
        echo "      about what the hardware actually holds — run as root for that."
        exit 0
    fi
    echo "self-test: $fails assertion(s) FAILED."
    exit 1
fi

# --- Report -----------------------------------------------------------------------------------

echo "# power-regs.sh — $(date -Is)"
echo "# card=$CARD slot=$pci_slot driver=$driver device=$device_id"
echo "# kernel=$(uname -r)"
echo

echo "== Raw =="
for name in "${REG_ORDER[@]}"; do
    printf '  %-24s %-10s = %s\n' "$name" "${REG[$name]}" "$(get "$name")"
done

if [[ $raw_only -eq 1 ]]; then
    exit 0
fi

echo
echo "== Units (PKG_POWER_SKU_UNIT $sku_unit) =="
printf '  PWR_UNIT    = %-2s -> power fields are in 1/%s W\n'  "$pwr_unit"    "$pwr_div"
printf '  ENERGY_UNIT = %-2s -> energy fields are in 1/%s J\n' "$energy_unit" "$(( 1 << energy_unit ))"
printf '  TIME_UNIT   = %-2s -> time fields are in 1/%s s\n'   "$time_unit"   "$(( 1 << time_unit ))"

echo
echo "== PKG_POWER_SKU — the hardware's own declared limits =="
printf '  PKG_TDP     = %-6s (%s)\n' "$pkg_tdp" "$(fmt_w "$(watts_mw "$pkg_tdp")")"
printf '  PKG_MIN_PWR = %-6s (%s)\n' "$pkg_min" "$(fmt_w "$(watts_mw "$pkg_min")")"
printf '  PKG_MAX_PWR = %-6s (%s)   <- the realistic ceiling for a raised PL1\n' \
       "$pkg_max" "$(fmt_w "$(watts_mw "$pkg_max")")"
printf '  PKG_MAX_WIN = 0x%02x\n' "$max_win"

echo
echo "== PKG_RAPL_LIMIT (PL1) =="
printf '  PWR_LIM_EN  = %s %s\n' "$pl1_en" \
       "$( if [[ $pl1_en -eq 1 ]]; then echo "(PL1 enforced)"; else echo "(PL1 DISABLED)"; fi )"
printf '  PWR_LIM_VAL = %-6s (%s)\n' "$pl1_val" "$(fmt_w "$(watts_mw "$pl1_val")")"
printf '  PWR_LIM     = 0x%04x  <- the 16 bits xe_hwmon_power_max_write() RMWs\n' "$pl1_pwr_lim"
printf '  PWR_LIM_TIME  x=%s y=%-2s -> tau = %d.%03d s\n' \
       "$tw_x" "$tw_y" $(( tau_ms / 1000 )) $(( tau_ms % 1000 ))

echo
echo "== PKG_RAPL_LIMIT upper dword (PL2) =="
echo "  NOTE: xe defines no fields here and never reads this dword. The layout below assumes"
echo "        the standard RAPL 64-bit form (PL2 val [46:32], en [47]). Treat as UNVERIFIED."
printf '  PL2_EN?     = %s\n' "$pl2_en"
printf '  PL2_VAL?    = %-6s (%s)\n' "$pl2_val" "$(fmt_w "$(watts_mw "$pl2_val")")"

echo
echo "== Frequencies =="
printf '  RP0 (fused max)     = %s MHz\n' "$rp0"
printf '  RP1                 = %s MHz\n' "$rp1"
printf '  RPn (fused min)     = %s MHz\n' "$rpn"
printf '  RPe (efficient)     = %s MHz\n' "$rpe"
printf '  RPa as xe reports   = %s MHz   [31:16] x50 — the value sysfs rpa_freq shows\n' "$rpa_driver"
printf '  RPa if mask were 8b = %s MHz   [31:24] x50\n' "$rpa_proposed"

echo
echo "== GT0_PERF_LIMIT_REASONS =="
printf '  live (masked 0xde3) = 0x%03x  %s\n' "$live" "$(decode_reasons "$live")"
printf '  upper half [31:16]  = 0x%04x  %s\n' "$upper" "$(decode_reasons "$upper")"
echo "  NOTE: xe masks reads with 0xde3 (xe_gt_throttle.c:93-97) and never touches the upper"
echo "        half, so sysfs cannot show it. On some Intel parts the upper bits are STICKY"
echo "        log bits — i.e. 'this reason fired at some point'. That is UNVERIFIED for DG2."
echo "        If it holds, a set bit here with a clear live bit means the reason has tripped"
echo "        since boot. Corroborate against a live sample before believing it."

# --- Plan 07 T2 hypothesis checks -------------------------------------------------------------
# RULES.md wants the refuting result written down before the data is collected. These were
# written into the plan first; the script only evaluates them.

echo
echo "== Plan 07 T2 predictions =="

check() {
    local verdict=$1 label=$2 detail=$3
    printf '  [%s] %s\n' "$verdict" "$label"
    if [[ -n "$detail" ]]; then printf '        %s\n' "$detail"; fi
}

if [[ $(( sku_lo )) -eq 0 ]]; then
    check "CONFIRMED" "PKG_POWER_SKU low dword reads 0" \
          "So power2_rated_max is hidden and the read-path clamp (min && max) is inert."
else
    check "REFUTED  " "PKG_POWER_SKU low dword is NON-zero ($sku_lo)" \
          "The read-path clamp IS active. Re-derive plan 07's Context before continuing."
fi

if [[ $(( pkg_max )) -ne 0 ]]; then
    check "INFO     " "PKG_MAX_PWR = $(fmt_w "$(watts_mw "$pkg_max")")" \
          "This is the value to aim at in T4."
else
    check "INFO     " "PKG_MAX_PWR reads 0 — SKU fuses unpopulated" \
          "The PCU's clamp target is not discoverable from MMIO; T4/T5 are the only route."
fi

if [[ $pl1_pwr_lim -eq $(( 0x80fa )) ]]; then
    check "CONFIRMED" "PWR_LIM = 0x80fa — 31.25 W with PL1 enabled, the value sysfs reports" ""
else
    check "INFO     " "PWR_LIM = $(printf '0x%04x' "$pl1_pwr_lim"), not the expected 0x80fa" \
          "Something changed it since boot, or the boot default differs."
fi

if [[ $rpa_bits_31_24 -eq $(( 0x31 )) ]] && [[ $rpa_bits_23_16 -eq 0 ]]; then
    check "CONFIRMED" "FREQ_INFO_REC [31:24] = 0x31, [23:16] = 0" \
          "RPa ratio is an 8-bit field at 31:24 -> real RPa = ${rpa_proposed} MHz (== RP0 ${rp0}). xe's RPA_MASK = REG_GENMASK(31,16) (xe_guc_pc.c:49) is too wide: upstream bug."
elif [[ $rpa_bits_23_16 -ne 0 ]]; then
    check "REFUTED  " "FREQ_INFO_REC [23:16] = $rpa_bits_23_16, non-zero" \
          "The field really is wider than 8 bits. The RPA_MASK theory is wrong — do not file it."
else
    check "PARTIAL  " "FREQ_INFO_REC [31:24] = $rpa_bits_31_24, [23:16] = 0" \
          "8-bit-field shape holds, but the ratio does not equal RP0/50 = $(( rp0 / 50 )). Investigate before concluding."
fi

if [[ $(( sku_unit )) -eq $(( 0x000a0e03 )) ]]; then
    check "CONFIRMED" "PKG_POWER_SKU_UNIT = 0x000a0e03, the canonical 0xa/0xe/0x3" ""
else
    check "INFO     " "PKG_POWER_SKU_UNIT = $sku_unit, not the expected 0x000a0e03" \
          "All watt/second conversions above use the values actually read, not the expected ones."
fi
