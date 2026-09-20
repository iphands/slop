#!/bin/bash
# Persist the btrfs readahead tuning for /main on noir.
#
# Why: /main is one PERC H710P RAID-6 virtual disk (10x 2TB SAS, 8 data spindles) and the
# controller caps requests at 256KB. btrfs' default 4MB readahead keeps only ~16 requests in
# flight, which leaves the spindles idle: ~350MB/s sequential. At 128MB readahead the same
# reads measured ~700MB/s (raw device ceiling ~775MB/s). nr_requests=256 caps the queue, so
# going higher than 131072 buys nothing.
#
# The knob lives in sysfs and resets every time /main is (re)mounted, hence the unit below.
#
# usage: main-readahead.sh show        print current value (changes nothing)
#        main-readahead.sh apply       set it now (runtime only)
#        main-readahead.sh install     write + enable a systemd unit that runs "apply"
#                                      whenever /main is mounted (boot and remounts)
#        main-readahead.sh uninstall   disable + remove that unit (value stays until remount)
#
# Tunables: MNT and KB can be overridden in the environment for apply/show.
# If Plex or other IO gets laggy while a big sequential read is running, try KB=32768.

set -eu

MNT=${MNT:-/main}
KB=${KB:-131072}
UNIT=main-readahead.service
UNIT_PATH=/etc/systemd/system/$UNIT
SELF=$(readlink -f "$0")

knob() {
    local uuid
    uuid=$(findmnt -no UUID --target "$MNT" -t btrfs) || { echo "$MNT is not a mounted btrfs filesystem" >&2; exit 1; }
    echo "/sys/fs/btrfs/$uuid/bdi/read_ahead_kb"
}

case ${1:-} in
show)
    f=$(knob)
    echo "$f = $(<"$f") KB"
    systemctl is-enabled "$UNIT" 2>/dev/null | sed "s/^/$UNIT: /" || echo "$UNIT: not installed"
    ;;
apply)
    f=$(knob)
    old=$(<"$f")
    echo "$KB" > "$f"
    echo "$f: $old -> $(<"$f") KB"
    ;;
install)
    # Tied to the mount unit rather than multi-user.target so a later umount/mount of /main
    # re-applies it too: WantedBy pulls it in when the mount starts, PartOf stops it when the
    # mount stops (so it is eligible to start again next time).
    mnt_unit=$(systemd-escape -p --suffix=mount "$MNT")
    cat > "$UNIT_PATH" <<EOF
[Unit]
Description=Raise btrfs readahead on $MNT (HW RAID needs a deep sequential queue)
Requires=$mnt_unit
After=$mnt_unit
PartOf=$mnt_unit

[Service]
Type=oneshot
RemainAfterExit=yes
Environment=MNT=$MNT KB=$KB
ExecStart=$SELF apply

[Install]
WantedBy=$mnt_unit
EOF
    systemctl daemon-reload
    systemctl enable "$UNIT"
    echo "installed $UNIT_PATH (runs: $SELF apply, KB=$KB, on every mount of $MNT)"
    echo "start it now with: systemctl start $UNIT"
    ;;
uninstall)
    systemctl disable --now "$UNIT" 2>/dev/null || true
    rm -f "$UNIT_PATH"
    systemctl daemon-reload
    echo "removed $UNIT (current readahead value is left as-is until $MNT is remounted)"
    ;;
*)
    sed -n '2,/^$/s/^# \{0,1\}//p' "$0"
    exit 1
    ;;
esac
