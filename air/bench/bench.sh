#!/bin/bash
# Thermal/throttle baseline for the M2 Air. Run before and after a cooling mod, then compare.py.
#   ./bench.sh --label stock            full run (~15 min incl. cooldowns)
#   ./bench.sh --label smoke --quick    ~2 min sanity run (short phases, no cooldown)
# Needs no root. Run on AC, battery not charging if you can (charging heats the chassis).
#
# No CPU die temp is exposed under Asahi, so throttling is detected from fixed-work throughput
# (sysbench --report-interval=1) + cpufreq + the "Heatpipe Power" rail; chassis temps are proxies.
# Output: results/<stamp>-<label>/{meta.txt,phases.csv,samples.csv,<phase>.log}
set -u

LABEL=run QUICK=0 COOL=1 FORCE=0 LONG=180 COOL_DELTA=2000 COOL_MAX=600 COOL_MIN=30
while [ $# -gt 0 ]; do
    case $1 in
        --label)       LABEL=$2; shift ;;
        --quick)       QUICK=1; COOL=0; LONG=20 ;;
        --duration)    LONG=$2; shift ;;
        --no-cooldown) COOL=0 ;;
        --cool-delta)  COOL_DELTA=$(printf '%.0f' "$(echo "$2*1000" | bc)"); shift ;;   # degC above start
        --cool-max)    COOL_MAX=$2; shift ;;
        --force)       FORCE=1 ;;
        -h|--help)     sed -n '2,9p' "$0"; exit 0 ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
    shift
done

for t in sysbench 7z taskset bc; do command -v $t >/dev/null || { echo "missing: $t" >&2; exit 1; }; done

# --- sensors: resolved by name/label, hwmon numbering moves between boots ---
hw() { local d; for d in /sys/class/hwmon/hwmon*; do [ "$(cat $d/name)" = "$1" ] && { echo $d; return; }; done; }
lbl() { local f; for f in $1/$2*_label; do [ "$(cat $f)" = "$3" ] && { echo ${f%_label}_input; return; }; done; }
SMC=$(hw macsmc_hwmon); NVME=$(hw nvme)
[ -n "$SMC" ] || { echo "no macsmc_hwmon" >&2; exit 1; }
T_NAND=$(lbl $SMC temp "NAND Flash Temperature");  T_BAT=$(lbl $SMC temp "Battery Hotspot")
T_CHG=$(lbl $SMC temp "Charge Regulator Temp");    T_WIFI=$(lbl $SMC temp "WiFi/BT Module Temp")
P_TOT=$(lbl $SMC power "Total System Power");      P_HEAT=$(lbl $SMC power "Heatpipe Power")
P_AC=$(lbl $SMC power "AC Input Power");           T_NVME=${NVME:+$NVME/temp1_input}
CF=/sys/devices/system/cpu; PS=/sys/class/power_supply
F_E=$CF/cpu0/cpufreq/scaling_cur_freq; F_P=$CF/cpu4/cpufreq/scaling_cur_freq
for f in $T_NAND $T_BAT $T_CHG $T_WIFI $P_TOT $P_HEAT $P_AC $F_E $F_P; do
    [ -r "${f:-/nonexistent}" ] || { echo "sensor unreadable: '$f'" >&2; exit 1; }
done
rd() { local v; read -r v < "$1" 2>/dev/null; echo "${v:-}"; }

# --- hygiene ---
load=$(cut -d' ' -f1 /proc/loadavg)
if [ "$(echo "$load > 0.5" | bc)" = 1 ] && [ $FORCE = 0 ]; then
    echo "loadavg $load > 0.5, machine not idle (--force to override)" >&2; exit 1
fi
status=$(rd $PS/macsmc-battery/status); ac=$(rd $PS/macsmc-ac/online); gov=$(rd $CF/cpu0/cpufreq/scaling_governor)
[ "$ac" = 1 ] || echo "WARN: not on AC; power limits differ, results won't compare to AC runs" >&2
[ "$status" = Charging ] && echo "WARN: battery charging adds chassis heat; prefer Full/Not charging" >&2

DIR=$(dirname "$(readlink -f "$0")")/results/$(date +%Y%m%d-%H%M%S)-$LABEL
mkdir -p "$DIR" || exit 1
START_T=$(rd $T_BAT)
{
    echo "label=$LABEL"
    echo "date=$(date -Is)"
    echo "kernel=$(uname -r)"
    echo "model=$(tr -d '\0' < /proc/device-tree/model)"
    echo "flags=quick=$QUICK cooldown=$COOL long=${LONG}s cool_delta_mc=$COOL_DELTA cool_max=${COOL_MAX}s"
    echo "governor=$gov"
    echo "max_khz_E=$(rd $CF/cpu0/cpufreq/scaling_max_freq)"
    echo "max_khz_P=$(rd $CF/cpu4/cpufreq/scaling_max_freq)"
    echo "ac_online=$ac"
    echo "battery_status=$status"
    echo "battery_pct=$(rd $PS/macsmc-battery/capacity)"
    echo "loadavg=$load"
    echo "start_t_nand_mc=$(rd $T_NAND)"
    echo "start_t_bat_mc=$START_T"
    echo "start_t_chg_mc=$(rd $T_CHG)"
    echo "start_t_wifi_mc=$(rd $T_WIFI)"
    echo "start_t_nvme_mc=$(rd ${T_NVME:-/dev/null})"
    echo "sysbench=$(sysbench --version)"
    echo "7z=$(7z 2>&1 | sed -n 2p)"
} > "$DIR/meta.txt"

# --- 1 Hz sampler, phase read from a file so one process covers the whole run ---
echo idle > "$DIR/.phase"
echo "epoch,t,phase,f_e_khz,f_p_khz,t_nand_mc,t_bat_mc,t_chg_mc,t_wifi_mc,t_nvme_mc,p_total_uw,p_heat_uw,p_ac_uw" > "$DIR/samples.csv"
sampler() {
    local t0=${EPOCHREALTIME/./} n=0 now ph
    while :; do
        read -r ph < "$DIR/.phase"
        echo "${EPOCHREALTIME%.*},$n,$ph,$(rd $F_E),$(rd $F_P),$(rd $T_NAND),$(rd $T_BAT),$(rd $T_CHG),$(rd $T_WIFI),$(rd ${T_NVME:-/dev/null}),$(rd $P_TOT),$(rd $P_HEAT),$(rd $P_AC)" >> "$DIR/samples.csv"
        n=$((n+1)); now=${EPOCHREALTIME/./}
        sleep "$(printf '0.%06d' $(( (t0 + n*1000000 - now) > 0 ? (t0 + n*1000000 - now) % 1000000 : 0 )))"
    done
}
sampler & SPID=$!
trap 'kill $SPID 2>/dev/null; rm -f "$DIR/.phase"' EXIT
trap 'exit 130' INT TERM

echo "phase,start_epoch,end_epoch,start_t_bat_mc,end_t_bat_mc,cooled_ok" > "$DIR/phases.csv"
COOLED=1
cooldown() {   # wait for chassis hotspot to return near the run's starting temp
    [ $COOL = 1 ] || { COOLED=na; return; }
    echo cool > "$DIR/.phase"; local s=$SECONDS t
    sleep $COOL_MIN
    while t=$(rd $T_BAT); [ "$t" -gt $((START_T + COOL_DELTA)) ]; do
        [ $((SECONDS - s)) -gt $COOL_MAX ] && { COOLED=0; echo "  cooldown timed out at $((t/1000)) C (start $((START_T/1000)) C)"; return; }
        sleep 5
    done
    COOLED=1
}
phase() {   # phase NAME CMD... ; stdout+stderr of CMD goes to NAME.log
    local name=$1; shift
    if [ "${PHASE_COOL:-0}" = 1 ]; then cooldown; else COOLED=na; fi
    echo "== $name: $*"
    local tin0=$DIR/$name.time_in_state.before s e
    cat $CF/cpu4/cpufreq/stats/time_in_state > "$tin0" 2>/dev/null
    echo "$name" > "$DIR/.phase"; s=${EPOCHREALTIME%.*}; local ts=$(rd $T_BAT)
    "$@" > "$DIR/$name.log" 2>&1
    e=${EPOCHREALTIME%.*}; echo idle > "$DIR/.phase"
    cat $CF/cpu4/cpufreq/stats/time_in_state > "$DIR/$name.time_in_state.after" 2>/dev/null
    echo "$name,$s,$e,$ts,$(rd $T_BAT),$COOLED" >> "$DIR/phases.csv"
}

SB="sysbench cpu --report-interval=1"
SHORT=30; [ $QUICK = 1 ] && SHORT=8
# st: single P-core, no thermal story; control that should not move after the mod
PHASE_COOL=0                phase st       taskset -c 4  $SB --threads=1 --time=$((SHORT*2/3)) run
PHASE_COOL=0                phase mt-short taskset -c 0-7 $SB --threads=8 --time=$SHORT run
PHASE_COOL=$COOL            phase p4-long  taskset -c 4-7 $SB --threads=4 --time=$LONG run
PHASE_COOL=$COOL            phase mt-long  taskset -c 0-7 $SB --threads=8 --time=$LONG run
# hot-state real-world + sanity (memory bandwidth should not move)
PHASE_COOL=0                phase 7z       7z b 1 -mmt=8
PHASE_COOL=0                phase mem      sysbench memory --threads=8 --memory-block-size=1M --memory-total-size=1T --time=10 --report-interval=1 run

echo cool > "$DIR/.phase"; [ $QUICK = 1 ] || sleep 30   # tail so the cooling curve after the last phase is captured
echo "done: $DIR"
