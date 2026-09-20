#!/bin/bash
# btrfs readahead sweep for /main on noir, driven by fio.
#
# For every read_ahead_kb value it runs four read-only fio tests:
#   seq    one buffered sequential stream, whole file          (a model load / nfsd streaming)
#   rand   buffered random reads, several jobs                 (does a big window hurt random IO?)
#   conc   several buffered sequential streams at once         (parallel shard loads)
#   mixed  one sequential stream + rate-limited random O_DIRECT reads on another file
#          (bulk read vs Plex/VM: how well the stream holds up, and how long the other IO waits)
#
# Read-only with respect to data: every fio call uses --readonly --allow_file_create=0 against
# files that already exist. It changes exactly one thing, the sysfs readahead knob, and restores
# the starting value on any exit (finish, abort, Ctrl-C, hangup). After a kill -9 run:
#   /root/main-readahead.sh apply
# Page cache: only the test files listed below are evicted; the O_DIRECT disturber target is not.
#
# usage:  /root/sweep_test.sh                       all tests, 17 values, 2 passes (~55 min)
#         PASSES=1 /root/sweep_test.sh              ~22 min
#         TESTS="seq mixed" /root/sweep_test.sh     subset
#         START=8192 END=131072 STEP=8192           range (these are the defaults; END always included)
#         VALUES_LIST="4096 8192 16384 ..."         explicit values instead of a range (KB, multiples of 4)
#         ALWAYS=4096                               injected into every grid (default 4096; ALWAYS= disables)
#         PASSES=20 MAX_HOURS=7 IDLE_WAIT_MIN=30 /root/sweep_test.sh    overnight: stops itself after 7h,
#                                                   prints a cumulative SUM block after every pass
# log:    /tmp/sweep_test.log (previous kept as .prev)   ->   tail -f /tmp/sweep_test.log

set -u

LOG=${LOG:-/tmp/sweep_test.log}
MD=/main/scratch/ai/llm/models/vllm/nvidia/Qwen3.8-Flash-Next-NVFP4
SEQ_F=${SEQ_F:-$MD/model-00007-of-00010.safetensors}
CONC_FILES=${CONC_FILES:-"$MD/model-00003-of-00010.safetensors $MD/model-00004-of-00010.safetensors $MD/model-00005-of-00010.safetensors $MD/model-00006-of-00010.safetensors"}
RAND_F=${RAND_F:-/main/scratch/ai/sd/models-bak/Stable-diffusion/flux_dev.safetensors}
DIST_F=${DIST_F:-$MD/model-fp8-mtp-ple.safetensors}      # O_DIRECT only, never evicted
MNT=${MNT:-/main}
# default grid = 8192..131072 step 8192 (lands on every power of two: 8M 16M 32M 64M 128M) + ALWAYS (4096) = 17 values
START=${START:-8192}; END=${END:-131072}; STEP=${STEP:-8192}
ALWAYS=${ALWAYS-4096}               # value(s) injected into every grid, range or list (the kernel default; ALWAYS= to disable)
PASSES=${PASSES:-2}
TESTS=${TESTS:-"seq rand conc mixed"}
CONC_SIZE=${CONC_SIZE:-3g}           # bytes each concurrent stream reads
RAND_SECS=${RAND_SECS:-12}; RAND_JOBS=${RAND_JOBS:-4}; RAND_BS=${RAND_BS:-64k}
DIST_IOPS=${DIST_IOPS:-50}; DIST_BS=${DIST_BS:-64k}
SLOW_MBPS=${SLOW_MBPS:-600}          # a 1-second sample below this counts as a "slow second"
IDLE_MIBPS=${IDLE_MIBPS:-20}         # background read rate above this = array not idle, wait
DISK_FRAC=${DISK_FRAC:-0.95}         # sequential tests are discarded unless this much came from disk
WRITES_MAX_MIB=${WRITES_MAX_MIB:-20}   # a run with more than this written to the array meanwhile is tagged NOISY:writes (kept in log, left out of SUM)
IDLE_WAIT_MIN=${IDLE_WAIT_MIN:-2}      # how long to wait for a quiet array before SKIPPING a run (overnight: 30)
MAX_HOURS=${MAX_HOURS:-0}              # stop starting new runs after this many hours (0 = no limit)
CACHED_MAX=${CACHED_MAX:-67108864}   # discard if more than this many bytes of the test files were still cached

trap '' PIPE                          # a dead terminal must never stop us from restoring the knob
log() { local l; l="$(date +%H:%M:%S) $*"; echo "$l" >> "$LOG"; echo "$l" 2>/dev/null || true; }

exec 9>/tmp/sweep_test.lock
flock -w 15 9 || { echo "another sweep_test.sh is already running (lock held for 15s)"; exit 1; }
[ -f "$LOG" ] && mv -f "$LOG" "$LOG.prev"
for d in /tmp/sweep_test.??????; do [ -d "$d" ] && rm -rf "$d"; done     # litter from a killed run
TMPD=$(mktemp -d /tmp/sweep_test.XXXXXX)

for c in fio jq fincore flock shuf; do command -v $c >/dev/null || { log "missing tool: $c"; exit 1; }; done
if [ -n "${RA_KNOB:-}" ]; then RA=$RA_KNOB; else
    uuid=$(findmnt -no UUID --target "$MNT" -t btrfs) || { log "cannot resolve $MNT as btrfs"; exit 1; }
    RA=/sys/fs/btrfs/$uuid/bdi/read_ahead_kb
fi
[ -w "$RA" ] || { log "no writable $RA (need root?)"; exit 1; }
for f in $SEQ_F $CONC_FILES $RAND_F $DIST_F; do [ -r "$f" ] || { log "test file not readable: $f"; exit 1; }; done
DEV=${DEV:-$(lsblk -no PKNAME "$(findmnt -no SOURCE --target "$MNT" | sed 's/\[.*//')" 2>/dev/null | head -1)}
DEV=${DEV:-sdb}
grep -qw "$DEV" /proc/diskstats || { log "device $DEV not in /proc/diskstats"; exit 1; }

ORIG=$(cat "$RA")
SPID=""
cleanup() {
    trap - EXIT
    [ -n "$SPID" ] && kill "$SPID" 2>/dev/null
    echo "$ORIG" > "$RA"
    log "RESTORED read_ahead_kb=$(cat "$RA")"
    rm -rf "$TMPD"
}
trap cleanup EXIT
trap 'log "# signalled, aborting"; exit 130' HUP INT TERM

if [ -n "${VALUES_LIST:-}" ]; then            # explicit list, space or comma separated
    VALUES=$(echo "$VALUES_LIST" | tr ',' ' ')
    for x in $VALUES; do
        case $x in ''|*[!0-9]*) log "VALUES_LIST: not a number: $x"; exit 1;; esac
        # the kernel stores the knob in 4KB pages; any other value reads back rounded and every run would be discarded
        [ $(( x % 4 )) -eq 0 ] && [ "$x" -gt 0 ] || { log "VALUES_LIST: $x is not a positive multiple of 4 KB"; exit 1; }
    done
    GRID="list: $(echo $VALUES)"
else
    VALUES=$(seq "$START" "$STEP" "$END")
    case " $(echo $VALUES) " in *" $END "*) ;; *) VALUES="$VALUES $END" ;; esac
    GRID="$START..$END step $STEP"
fi
for x in $(echo "$ALWAYS" | tr ',' ' '); do
    case $x in ''|*[!0-9]*) log "ALWAYS: not a number: $x"; exit 1;; esac
    [ $(( x % 4 )) -eq 0 ] && [ "$x" -gt 0 ] || { log "ALWAYS: $x is not a positive multiple of 4 KB"; exit 1; }
    case " $(echo $VALUES) " in *" $x "*) ;; *) VALUES="$x $VALUES"; GRID="$GRID + $x" ;; esac
done
VALUES=$(echo $VALUES | tr ' ' '\n' | sort -n -u | tr '\n' ' ')
NVAL=$(echo $VALUES | wc -w); NTEST=$(echo $TESTS | wc -w)

now_us()  { echo "${EPOCHREALTIME/./}"; }
sectors() { awk -v d="$DEV" '$3==d{print $6}' /proc/diskstats; }
st()      { awk -v d="$DEV" '$3==d{print $4, $6, $7, $8, $10}' /proc/diskstats; }   # rIOs rsect rms wIOs wsect

idle() {
    local i a b r
    local tries=$(( IDLE_WAIT_MIN * 20 )); [ "$tries" -lt 1 ] && tries=1
    for i in $(seq 1 "$tries"); do
        a=$(sectors); sleep 3; b=$(sectors)
        r=$(( (b - a) * 512 / 3 / 1048576 ))
        [ "$r" -le "$IDLE_MIBPS" ] && return 0
        log "# busy: ${r} MiB/s background read on $DEV, waiting ($i/$tries)"
    done
    return 1
}
sampler() { while :; do echo "$(now_us) $(sectors)" >> "$1"; sleep 1; done; }
persec() {   # min per-second MB/s, count of slow seconds (first/last interval dropped)
    awk -v slow="$SLOW_MBPS" 'NR>1{ n++; r[n]=($2-ps)*512/1e6/(($1-pt)/1e6) } {pt=$1; ps=$2}
        END{ min=-1; c=0; for(i=2;i<n;i++){ if(min<0||r[i]<min)min=r[i]; if(r[i]<slow)c++ }
             if(min<0)min=0; printf "%.0f %d\n", min, c }' "$1"
}
evict() {    # drop the given files from page cache, print how many bytes of them are still cached
    local f tot=0 r
    for f in "$@"; do dd if="$f" iflag=nocache count=0 status=none 2>/dev/null; done
    for f in "$@"; do r=$(fincore -nb -o RES "$f" 2>/dev/null | tr -d ' '); tot=$(( tot + ${r:-0} )); done
    echo "$tot"
}

# best-effort: which processes dirtied the most data while we ran (any filesystem, not just /main)
snap_writers() { for d in /proc/[0-9]*; do [ -r "$d/io" ] || continue; w=$(awk '/^write_bytes/{print $2}' "$d/io" 2>/dev/null) || continue
                   echo "${d#/proc/} $(tr -d ' ' < "$d/comm" 2>/dev/null) ${w:-0}"; done > "$1" 2>/dev/null; }
report_writers() { snap_writers "$TMPD/w1"; awk 'NR==FNR{a[$1]=$3; next} ($1 in a) && $3-a[$1] > 10485760 {printf "%s pid=%s mib=%.0f\n", $2, $1, ($3-a[$1])/1048576}' "$TMPD/w0" "$TMPD/w1" \
                   | sort -t= -k3 -n -r | head -8 | while read -r l; do log "WRITER since_start $l"; done; }

# fadvise_hint=0 matters: by default fio declares sequential access, which makes Linux DOUBLE the
# readahead window - every value would silently be tested at 2x, unlike nfsd/dd.
FIO="fio --readonly --allow_file_create=0 --output-format=json --percentile_list=50:95:99 --fadvise_hint=0 --ioengine=psync"
DIST_JOB="--name=dist --filename=$DIST_F --rw=randread --bs=$DIST_BS --direct=1 --invalidate=0 --iodepth=1 --rate_iops=$DIST_IOPS --norandommap --randrepeat=0 --time_based"

run_fio() {  # $1 = test name, json lands in $TMPD/fio.json
    local i=0 f args=""
    case $1 in
    seq)   $FIO --output="$TMPD/fio.json" --name=seq --filename="$SEQ_F" --rw=read --bs=1M --direct=0 --invalidate=1 ;;
    rand)  $FIO --output="$TMPD/fio.json" --name=rand --filename="$RAND_F" --rw=randread --bs="$RAND_BS" --direct=0 --invalidate=1 \
                --numjobs="$RAND_JOBS" --group_reporting --time_based --runtime="$RAND_SECS" --norandommap --randrepeat=0 ;;
    conc)  for f in $CONC_FILES; do i=$((i+1)); args="$args --name=c$i --filename=$f"; done
           $FIO --output="$TMPD/fio.json" --rw=read --bs=1M --direct=0 --invalidate=1 --size="$CONC_SIZE" $args ;;
    mixed) $FIO --output="$TMPD/fio.json" --name=seq --filename="$SEQ_F" --rw=read --bs=1M --direct=0 --invalidate=1 --exitall \
                $DIST_JOB --runtime=600 ;;
    esac
}
files_of() { case $1 in seq|mixed) echo "$SEQ_F";; rand) echo "$RAND_F";; conc) echo "$CONC_FILES";; esac; }

# fio json -> "mbps secs total_mib streams_min_mbps iops p50 p95 p99"   ("-" = not applicable)
parse() {
    case $1 in
    seq)   jq -r '.jobs[]|select(.jobname=="seq").read | [(.io_bytes/(.runtime/1000)/1e6), (.runtime/1000), (.io_bytes/1048576), "-", "-", "-", "-", "-"] | map(tostring) | join(" ")' "$TMPD/fio.json" ;;
    rand)  jq -r '.jobs[0].read | [(.bw_bytes/1e6), (.runtime/1000), (.io_bytes/1048576), "-", .iops, (.clat_ns.percentile["50.000000"]/1e6), (.clat_ns.percentile["95.000000"]/1e6), (.clat_ns.percentile["99.000000"]/1e6)] | map(tostring) | join(" ")' "$TMPD/fio.json" ;;
    conc)  jq -r '[.jobs[].read] | (map(.io_bytes)|add) as $b | (map(.runtime)|max) as $r | [($b/($r/1000)/1e6), ($r/1000), ($b/1048576), (map(.bw_bytes)|min/1e6), "-", "-", "-", "-"] | map(tostring) | join(" ")' "$TMPD/fio.json" ;;
    mixed) jq -r '(.jobs[]|select(.jobname=="seq").read) as $s | (.jobs[]|select(.jobname=="dist").read) as $d | [($s.io_bytes/($s.runtime/1000)/1e6), ($s.runtime/1000), ($s.io_bytes/1048576), "-", $d.iops, ($d.clat_ns.percentile["50.000000"]/1e6), ($d.clat_ns.percentile["95.000000"]/1e6), ($d.clat_ns.percentile["99.000000"]/1e6)] | map(tostring) | join(" ")' "$TMPD/fio.json" ;;
    esac
}

total=$(( NVAL * PASSES * NTEST ))
log "# sweep_test (fio) start: values=$NVAL ($GRID) passes=$PASSES tests='$TESTS' runs=$total"
log "# knob=$RA start_value=$ORIG dev=$DEV $(fio --version)"
log "# seq/mixed file=$(basename "$SEQ_F")  rand file=$(basename "$RAND_F") (${RAND_JOBS}x$RAND_BS buffered, ${RAND_SECS}s)"
log "# conc = $(echo $CONC_FILES | wc -w) streams x $CONC_SIZE  disturber = $DIST_IOPS iops x $DIST_BS O_DIRECT on $(basename "$DIST_F")"

# disturber alone on an idle array = best-case latency for other IO
idle || { log "# ABORT: array busy before start"; exit 2; }
if $FIO --output="$TMPD/fio.json" $DIST_JOB --runtime=10 >/dev/null 2>"$TMPD/fio.err"; then
    log "BASE idle_array $(jq -r '.jobs[0].read | "dist_iops=\(.iops|floor) lat_p50_ms=\(.clat_ns.percentile["50.000000"]/1e6) lat_p95_ms=\(.clat_ns.percentile["95.000000"]/1e6) lat_p99_ms=\(.clat_ns.percentile["99.000000"]/1e6)"' "$TMPD/fio.json")"
else
    log "# ABORT: fio failed on the baseline: $(head -c 300 "$TMPD/fio.err")"; exit 3
fi

summary() {
    log "# SUMMARY after pass $1 (status=ok only; noisy = runs left out because of concurrent writes). mbps: median/min over passes; slow = avg slow seconds; iops + lat = medians (rand: its own reads, mixed: the disturber)"
    echo "$RESULTS" | awk '
        function med(arr, n,   i, j, t) { for(i=1;i<=n;i++) for(j=i+1;j<=n;j++) if (arr[j]<arr[i]) {t=arr[i];arr[i]=arr[j];arr[j]=t}
                                           return (n%2) ? arr[(n+1)/2] : (arr[n/2]+arr[n/2+1])/2 }
        /^RUN/ { split("", kv); for (i=2;i<=NF;i++) { split($i, a, "="); kv[a[1]]=a[2] }
                 if (kv["status"] ~ /^NOISY/) noisy[kv["test"] SUBSEP kv["ra"]]++
                 if (kv["status"] != "ok") next
                 key=kv["test"] SUBSEP kv["ra"]; n[key]++; c=n[key]
                 mb[key,c]=kv["mbps"]+0; io[key,c]=kv["iops"]+0; p50[key,c]=kv["lat_p50_ms"]+0; p95[key,c]=kv["lat_p95_ms"]+0; p99[key,c]=kv["lat_p99_ms"]+0
                 slow[key]+=kv["slow_secs"]; inf[key]+=kv["inflight"]; ratio[key]+=kv["disk_ratio"]; tests[kv["test"]]=1; ras[kv["ra"]]=1 }
        END { for (t in tests) for (ra in ras) { key=t SUBSEP ra; c=n[key]; if (!c) continue
                for(i=1;i<=c;i++){ A[i]=mb[key,i]; B[i]=io[key,i]; C[i]=p50[key,i]; D[i]=p95[key,i]; E[i]=p99[key,i] }
                mn=A[1]; for(i=2;i<=c;i++) if (A[i]<mn) mn=A[i]
                printf "%s %d n=%d noisy=%d mbps_median=%.0f mbps_min=%.0f slow_secs_avg=%.1f inflight=%.0f disk_ratio=%.2f iops=%.0f lat_p50_ms=%.1f lat_p95_ms=%.1f lat_p99_ms=%.1f\n",
                       t, ra, c, noisy[key]+0, med(A,c), mn, slow[key]/c, inf[key]/c, ratio[key]/c, med(B,c), med(C,c), med(D,c), med(E,c) } }' | sort -k1,1 -k2,2n | while read -r l; do log "SUM pass<=$1 $l"; done
}

RESULTS=""; runno=0; T_START=$SECONDS; stop=0
snap_writers "$TMPD/w0"
for pass in $(seq 1 "$PASSES"); do
    # shuffle (value,test) pairs across the whole pass: a 2-minute write burst then hits random
    # cells instead of wiping out all four tests of one value
    for pair in $(for ra in $VALUES; do for t in $TESTS; do echo "$ra:$t"; done; done | shuf); do
        ra=${pair%%:*}; t=${pair##*:}
        for once in 1; do
            runno=$((runno + 1))
            if [ "$MAX_HOURS" != 0 ] && [ $(( SECONDS - T_START )) -ge $(awk -v h="$MAX_HOURS" 'BEGIN{printf "%d", h*3600}') ]; then
                log "# time limit MAX_HOURS=$MAX_HOURS reached, stopping"; stop=1; break
            fi
            idle || { log "RUN n=$runno/$total pass=$pass test=$t ra=$ra status=SKIP:arraybusy"; continue; }
            cached=$(evict $(files_of "$t"))
            echo "$ra" > "$RA"
            : > "$TMPD/sec"; rm -f "$TMPD/fio.json"
            sampler "$TMPD/sec" & SPID=$!
            s1=$(st); t0=$(now_us)
            run_fio "$t" >/dev/null 2>"$TMPD/fio.err"; rc=$?
            t1=$(now_us); s2=$(st); now=$(cat "$RA")
            kill "$SPID" 2>/dev/null; wait "$SPID" 2>/dev/null; SPID=""
            read -r minsec slowsecs < <(persec "$TMPD/sec")
            if [ $rc -ne 0 ] || ! fields=$(parse "$t" 2>/dev/null) || [ -z "$fields" ]; then
                line="RUN n=$runno/$total pass=$pass test=$t ra=$ra status=DISCARD:fiofail,rc=$rc,$(head -c 120 "$TMPD/fio.err" | tr ' \n' '__')"
                log "$line"; continue
            fi
            line=$(echo "$s1 $s2 $t0 $t1 $fields" | awk -v ra="$ra" -v p="$pass" -v t="$t" -v c="$cached" -v now="$now" \
                -v minsec="$minsec" -v slowsecs="$slowsecs" -v k="$runno" -v tot="$total" -v frac="$DISK_FRAC" -v cmax="$CACHED_MAX" -v wmax="$WRITES_MAX_MIB" '{
                io=$6-$1; el=($12-$11)/1e6; disk=($7-$2)*512/1048576; lat=(io>0)?($8-$3)/io:0; wmib=($10-$5)*512/1048576
                mbps=$13; secs=$14; tmib=$15; smin=$16; iops=$17; p50=$18; p95=$19; p99=$20
                ratio=(tmib>0)?disk/tmib:0
                why=""; if (now != ra) why=why"knobchanged,"; if (c > cmax) why=why"cached,"
                if (t != "rand" && ratio < frac) why=why"notfromdisk,"; if (t == "rand" && ratio < 0.5) why=why"cachehits,"
                status=(why!="")?"DISCARD:"why:((wmib > wmax)?"NOISY:writes":"ok")
                if (t == "rand") { minsec="-"; slowsecs="-" }
                printf "RUN n=%d/%d pass=%d test=%s ra=%d mbps=%.1f secs=%.1f minsec_mbps=%s slow_secs=%s inflight=%.1f lat_ms=%.1f fio_mib=%.0f disk_mib=%.0f disk_ratio=%.2f other_writes_mib=%.1f streams_min_mbps=%s iops=%s lat_p50_ms=%s lat_p95_ms=%s lat_p99_ms=%s status=%s",
                    k, tot, p, t, ra, mbps, secs, minsec, slowsecs, (el>0)?io/el*lat/1000:0, lat, tmib, disk, ratio, wmib,
                    (smin=="-")?"-":sprintf("%.1f",smin), (iops=="-")?"-":sprintf("%.1f",iops),
                    (p50=="-")?"-":sprintf("%.1f",p50), (p95=="-")?"-":sprintf("%.1f",p95), (p99=="-")?"-":sprintf("%.1f",p99), status }')
            log "$line"
            RESULTS+="$line"$'\n'
        done
        [ "$stop" = 1 ] && break
    done
    summary "$pass"
    report_writers
    [ "$stop" = 1 ] && break
done

log "# done"
