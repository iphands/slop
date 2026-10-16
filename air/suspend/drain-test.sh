#!/bin/bash
# Measure s2idle drain. Run unplugged:  sudo ./drain-test.sh [minutes=60]
# Wakes itself? No (macsmc-rtc has no wakealarm) — open the lid/press a key after the time.
# Records battery before suspend, then prints the delta on resume. Compare with/without the hook.
B=/sys/class/power_supply/macsmc-battery
[ "$(cat $B/status)" = Discharging ] || { echo "plug OUT the charger first"; exit 1; }
e0=$(cat $B/energy_now); t0=$(date +%s)
echo "before: $(cat $B/capacity)%  energy_now=$e0 (uWh)  hook installed: $([ -x /usr/lib/systemd/system-sleep/s2idle-trim ] && echo yes || echo no)"
systemctl suspend
e1=$(cat $B/energy_now); t1=$(date +%s); h=$(echo "scale=3;($t1-$t0)/3600"|bc)
echo "after:  $(cat $B/capacity)%  energy_now=$e1  elapsed=${h}h"
echo "drain: $(echo "scale=2;($e0-$e1)/1000"|bc) mWh  =  $(echo "scale=1;($e0-$e1)/1000/$h"|bc) mW avg (incl. resume)"
