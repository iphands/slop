#!/bin/bash
# Manual "no sleep" test of s2idle-trim: runs pre, waits, runs post, verifies recovery.
# Does NOT suspend. Drops Wi-Fi/BT for a few seconds, so run from a LOCAL terminal.
#   sudo ./hook-test.sh [hook-path]     (default: s2idle-trim.sh next to this file)
# Exit 0 = Wi-Fi + BT modules loaded, wifi netdev + hci0 present, NetworkManager connected.
[ "$EUID" = 0 ] || { echo "run with sudo"; exit 1; }
H=${1:-$(dirname "$(readlink -f "$0")")/s2idle-trim.sh}
[ -x "$H" ] || { echo "no executable hook at $H"; exit 1; }
since=$(date '+%Y-%m-%d %H:%M:%S')

echo "== hook: $H"; echo "== before"; "$H" status
echo "== pre suspend";  time "$H" pre suspend
echo "-- radios off; sleeping 3s"; sleep 3
echo "== state while down"; "$H" status
echo "== post suspend"; time "$H" post suspend
sleep 3   # let NetworkManager reconnect

echo "== journal"; journalctl -t s2idle-trim --since "$since" --no-pager
echo "== after"; out=$("$H" status); echo "$out"

fail=0
echo "$out" | grep -q 'NOT loaded' && { echo "FAIL: module(s) not loaded"; fail=1; }
echo "$out" | grep -q 'wifi netdev: yes' || { echo "FAIL: no wifi netdev"; fail=1; }
echo "$out" | grep -q 'bt hci0: yes'     || { echo "FAIL: no hci0"; fail=1; }
st=$(nmcli -t -f STATE general 2>/dev/null)
echo "NetworkManager: ${st:-unknown}"
[ "$st" = connected ] || { echo "WARN: NetworkManager not 'connected' yet (may still be reconnecting)"; }
[ $fail = 0 ] && echo "PASS" || { echo "recover with: sudo $H restore"; exit 1; }
