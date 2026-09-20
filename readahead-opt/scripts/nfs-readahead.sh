#!/bin/bash
# Set client-side readahead for every mounted NFS filesystem.
# Runtime only: resets when the share is remounted or the machine reboots.
#
# usage: sudo ./nfs-readahead.sh [KB]      (default 16384, kernel default is 128)
#        ./nfs-readahead.sh show           (just print current values, no root needed)

KB=${1:-16384}

if [[ $KB != show && ! $KB =~ ^[0-9]+$ ]]; then
    echo "usage: $0 [KB|show]" >&2
    exit 1
fi

if [[ $KB != show && $EUID -ne 0 ]]; then
    echo "need root to write /sys/class/bdi/*/read_ahead_kb (try: sudo $0 $KB)" >&2
    exit 1
fi

found=0
while read -r target source; do
    bdi=$(mountpoint -d "$target") || continue
    f=/sys/class/bdi/$bdi/read_ahead_kb
    [[ -e $f ]] || { echo "skip $target: no $f" >&2; continue; }
    found=1
    old=$(<"$f")
    if [[ $KB == show ]]; then
        printf '%-28s %-8s %6s KB  %s\n' "$target" "$bdi" "$old" "$source"
    else
        echo "$KB" > "$f"
        printf '%-28s %-8s %6s -> %6s KB  %s\n' "$target" "$bdi" "$old" "$(<"$f")" "$source"
    fi
done < <(findmnt -rn -t nfs,nfs4 -o TARGET,SOURCE)

[[ $found -eq 1 ]] || { echo "no NFS mounts found" >&2; exit 1; }
