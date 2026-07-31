#!/usr/bin/env bash
# gpu-survey.sh — unprivileged whole-session GPU sampler for Intel Arc on the `xe` driver.
#
# Why this exists: Plan 01 T5 was written around `gputop -J -s 200`. On this box `gputop`
# (igt-gpu-tools 2.4) accepts only -h/-d/-n, has no machine-readable output, and writes
# nothing but ANSI clear-screen codes when redirected. See context/pitfalls.md.
#
# Everything below is read from /proc and /sys and needs no root:
#   engine busy %  <- /proc/<pid>/fdinfo/<drm fd>  drm-cycles-* / drm-total-cycles-*
#   VRAM           <- same, drm-total-vram0 / drm-resident-vram0
#   clocks         <- /sys/class/drm/card0/device/tile0/gt0/freq0/{act,cur,rp0}_freq
#   throttle       <- .../freq0/throttle/{reasons,reason_*}      [the Rule D gate]
#   temp/fan/power <- .../device/hwmon/hwmon*/  (power derived from energy2_input deltas)
#
# Emits CSV to stdout or --out. Ctrl-C stops and prints a run verdict.
#
# NB on style: this script avoids the `[[ cond ]] && assign` idiom throughout. Under
# `set -e` such a statement returns non-zero when the condition is false and silently
# kills the script — which here would mean a capture that stops early while looking like
# it succeeded. Explicit `if` blocks only.

set -euo pipefail

CARD_DEV=/sys/class/drm/card0/device
FREQ_DIR="$CARD_DEV/tile0/gt0/freq0"
THROTTLE_DIR="$FREQ_DIR/throttle"
ENGINES=(rcs bcs vcs vecs ccs)

interval=0.2
out=""
target_pid=""
match=""
label=""
start_time=""

usage() {
    cat <<'EOF'
Usage: gpu-survey.sh [--pid N | --match PATTERN] [--interval SEC] [--out FILE] [--label TXT]

  --pid N         sample this pid's DRM clients
  --match PATTERN pgrep -f pattern to resolve the pid (waits for it to appear)
  --interval SEC  sampling period (default 0.2 = 5 Hz)
  --out FILE      write CSV here instead of stdout
  --label TXT     free-text label recorded in the header comment

With neither --pid nor --match, samples GPU-wide sysfs only (clocks/throttle/thermals);
per-engine busy and VRAM need a pid, because they come from that process's fdinfo.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --pid)      target_pid="$2"; shift 2 ;;
        --match)    match="$2"; shift 2 ;;
        --interval) interval="$2"; shift 2 ;;
        --out)      out="$2"; shift 2 ;;
        --label)    label="$2"; shift 2 ;;
        -h|--help)  usage; exit 0 ;;
        *) echo "gpu-survey: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ ! -d $FREQ_DIR ]]; then
    echo "gpu-survey: $FREQ_DIR missing — is this an xe-driven Arc?" >&2
    exit 1
fi

read_first() {
    if [[ -r $1 ]]; then
        head -1 "$1" 2>/dev/null || true
    fi
}

# --- resolve target pid -------------------------------------------------------------
if [[ -n $match && -z $target_pid ]]; then
    echo "gpu-survey: waiting for a process matching '$match' ..." >&2
    for _ in $(seq 1 600); do
        target_pid=$(pgrep -f -- "$match" 2>/dev/null | head -1 || true)
        if [[ -n $target_pid ]]; then
            break
        fi
        sleep 0.5
    done
    if [[ -z $target_pid ]]; then
        echo "gpu-survey: no process matched '$match' after 5 min" >&2
        exit 1
    fi
    echo "gpu-survey: attached to pid $target_pid" >&2
fi

# --- discover hwmon files (names vary by kernel) -------------------------------------
hwmon=""
for h in "$CARD_DEV"/hwmon/hwmon*; do
    if [[ -d $h ]]; then
        hwmon="$h"
        break
    fi
done

energy_file=""; temp_file=""; fan_file=""; power_limit="NA"
if [[ -n $hwmon ]]; then
    for e in energy2_input energy1_input; do
        if [[ -r "$hwmon/$e" ]]; then
            energy_file="$hwmon/$e"
            break
        fi
    done
    for t in temp2_input temp1_input temp3_input; do
        if [[ -r "$hwmon/$t" ]]; then
            temp_file="$hwmon/$t"
            break
        fi
    done
    if [[ -r "$hwmon/fan1_input" ]]; then
        fan_file="$hwmon/fan1_input"
    fi
    for p in power2_max power1_max; do
        if [[ -r "$hwmon/$p" ]]; then
            power_limit="$(read_first "$hwmon/$p")"
            break
        fi
    done
fi

# --- one fdinfo sample --------------------------------------------------------------
# Sums per-engine cycles across the pid's DRM clients, DEDUPLICATED BY drm-client-id.
# A single process holds several DRM fds with distinct client-ids; summing per-fd
# double-counts. Verified with vkcube: 3 DRM fds, client-ids 777/778/782.
sample_fdinfo() {
    local pid=$1 files=() f fd tgt
    if [[ ! -d /proc/$pid/fdinfo ]]; then
        return 1
    fi
    for f in /proc/"$pid"/fdinfo/*; do
        if [[ ! -e $f ]]; then
            continue
        fi
        fd=${f##*/}
        tgt=$(readlink "/proc/$pid/fd/$fd" 2>/dev/null) || continue
        case "$tgt" in */dri/*) files+=("$f") ;; esac
    done
    if [[ ${#files[@]} -eq 0 ]]; then
        return 1
    fi

    awk '
    function flush(   e) {
        if (cid != "" && !(cid in seen)) {
            seen[cid] = 1
            for (e in cyc)  tot_cyc[e] += cyc[e]
            # total-cycles is a per-engine wall-clock reference, identical in rate across
            # clients, so take the max rather than summing it.
            for (e in tcyc) if (tcyc[e] > tot_tcyc[e]) tot_tcyc[e] = tcyc[e]
            vram_total += vt; vram_res += vr
        }
        cid = ""; split("", cyc); split("", tcyc); vt = 0; vr = 0
    }
    function to_kib(val, unit) {
        if (unit == "MiB") return val * 1024
        if (unit == "GiB") return val * 1024 * 1024
        if (unit == "B")   return val / 1024
        return val   # KiB, or unitless 0
    }
    FNR == 1 { flush() }
    /^drm-client-id:/      { cid = $2 }
    /^drm-cycles-/         { e = $1; sub(/^drm-cycles-/, "", e);       sub(/:$/, "", e); cyc[e]  = $2 }
    /^drm-total-cycles-/   { e = $1; sub(/^drm-total-cycles-/, "", e); sub(/:$/, "", e); tcyc[e] = $2 }
    /^drm-total-vram0:/    { vt = to_kib($2, $3) }
    /^drm-resident-vram0:/ { vr = to_kib($2, $3) }
    END {
        flush()
        n = split(ENGINES, elist, " ")
        for (i = 1; i <= n; i++) {
            e = elist[i]
            printf "%s %.0f %.0f\n", e, (e in tot_cyc ? tot_cyc[e] : 0), (e in tot_tcyc ? tot_tcyc[e] : 0)
        }
        printf "VRAM %.0f %.0f\n", vram_total, vram_res
    }' ENGINES="${ENGINES[*]}" "${files[@]}"
}

# --- output setup -------------------------------------------------------------------
emit() {
    if [[ -n $out ]]; then
        printf '%s\n' "$*" >>"$out"
    else
        printf '%s\n' "$*"
    fi
}

if [[ -n $out ]]; then
    mkdir -p "$(dirname "$out")"
    : >"$out"
    echo "gpu-survey: writing $out" >&2
fi

emit "# gpu-survey.sh  host=$(hostname)  date=$(date -Is)  interval=${interval}s"
emit "# label=${label:-none}  pid=${target_pid:-none}"
emit "# rp0_freq=$(read_first "$FREQ_DIR/rp0_freq") MHz  power_limit_uw=${power_limit}"

header="t_s"
for e in "${ENGINES[@]}"; do
    header+=",busy_${e}_pct"
done
header+=",vram_total_mib,vram_resident_mib,act_freq_mhz,cur_freq_mhz,rp0_freq_mhz"
header+=",throttle_reasons,throttled,temp_c,fan_rpm,power_w"
emit "$header"

# --- sampling loop ------------------------------------------------------------------
declare -A prev_cyc prev_tcyc
prev_energy=""
prev_time=""
throttle_trips=0
samples=0
first_throttle_reason=""

finish() {
    trap - INT TERM
    echo >&2
    if [[ $throttle_trips -gt 0 ]]; then
        echo "gpu-survey: THROTTLED — $throttle_trips/$samples samples reported a throttle" >&2
        echo "gpu-survey: reason (first: $first_throttle_reason)." >&2
        echo "gpu-survey: Per RULES.md Rule D.5 this run is INVALID. Discard and re-measure;" >&2
        echo "gpu-survey: do not caveat it. Let the card settle, then re-run." >&2
        emit "# VERDICT: THROTTLED in $throttle_trips/$samples samples (first: $first_throttle_reason) — RUN INVALID per Rule D.5"
    else
        echo "gpu-survey: no throttle across $samples samples — valid on that axis." >&2
        emit "# VERDICT: no throttle across $samples samples"
    fi
    if [[ -n $out ]]; then
        echo "gpu-survey: wrote $out" >&2
    fi
    exit 0
}
trap finish INT TERM

while true; do
    now=$(date +%s.%N)
    if [[ -z $start_time ]]; then
        start_time="$now"
    fi

    act=$(read_first "$FREQ_DIR/act_freq")
    cur=$(read_first "$FREQ_DIR/cur_freq")
    rp0=$(read_first "$FREQ_DIR/rp0_freq")

    reasons=$(read_first "$THROTTLE_DIR/reasons")
    if [[ -z $reasons ]]; then
        reasons="unknown"
    fi

    throttled=0
    if [[ $reasons != "none" && $reasons != "unknown" ]]; then
        throttled=1
        throttle_trips=$((throttle_trips + 1))
        if [[ -z $first_throttle_reason ]]; then
            first_throttle_reason="$reasons"
        fi
        echo "gpu-survey: WARNING throttle active: $reasons" >&2
    fi

    temp=""; fan=""; power=""
    if [[ -n $temp_file ]]; then
        temp=$(awk '{printf "%.1f", $1/1000}' "$temp_file" 2>/dev/null || echo "")
    fi
    if [[ -n $fan_file ]]; then
        fan=$(read_first "$fan_file")
    fi
    if [[ -n $energy_file ]]; then
        energy=$(read_first "$energy_file")
        if [[ -n $prev_energy && -n $prev_time && -n $energy ]]; then
            power=$(awk -v e1="$prev_energy" -v e2="$energy" -v t1="$prev_time" -v t2="$now" \
                'BEGIN { dt = t2 - t1; if (dt > 0) printf "%.2f", (e2 - e1) / 1e6 / dt }')
        fi
        prev_energy="$energy"
    fi

    busy_fields=""
    vram_total_mib=""
    vram_res_mib=""
    if [[ -n $target_pid ]]; then
        fd_sample=""
        if ! fd_sample=$(sample_fdinfo "$target_pid" 2>/dev/null); then
            if [[ ! -d /proc/$target_pid ]]; then
                echo "gpu-survey: target pid $target_pid exited — stopping." >&2
                finish
            fi
            fd_sample=""
        fi
        if [[ -n $fd_sample ]]; then
            while read -r key v1 v2; do
                if [[ -z $key ]]; then
                    continue
                fi
                if [[ $key == VRAM ]]; then
                    vram_total_mib=$(awk -v k="$v1" 'BEGIN { printf "%.1f", k/1024 }')
                    vram_res_mib=$(awk -v k="$v2" 'BEGIN { printf "%.1f", k/1024 }')
                else
                    pc=""
                    if [[ -n ${prev_cyc[$key]:-} ]]; then
                        pc=$(awk -v c1="${prev_cyc[$key]}" -v c2="$v1" \
                                 -v t1="${prev_tcyc[$key]:-0}" -v t2="$v2" \
                            'BEGIN { dt = t2 - t1; if (dt > 0) printf "%.2f", (c2 - c1) * 100.0 / dt; else printf "0.00" }')
                    fi
                    busy_fields+=",${pc}"
                    prev_cyc[$key]="$v1"
                    prev_tcyc[$key]="$v2"
                fi
            done <<<"$fd_sample"
        fi
    fi
    if [[ -z $busy_fields ]]; then
        for _ in "${ENGINES[@]}"; do
            busy_fields+=","
        done
    fi

    t_rel=$(awk -v a="$now" -v s="$start_time" 'BEGIN { printf "%.3f", a - s }')

    emit "${t_rel}${busy_fields},${vram_total_mib},${vram_res_mib},${act},${cur},${rp0},${reasons},${throttled},${temp},${fan},${power}"

    prev_time="$now"
    samples=$((samples + 1))
    sleep "$interval"
done
