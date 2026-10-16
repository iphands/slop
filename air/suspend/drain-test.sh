#!/bin/bash
# Measure s2idle drain. Run unplugged:  sudo ./drain-test.sh
# Wakes itself? No (macsmc-rtc has no wakealarm) — open the lid/press a key after 1h+.
# Records battery before suspend, then prints the delta on resume. Compare with/without the hook.
#
# `systemctl suspend` returns as soon as the request is queued, NOT after resume. So we wait
# for the kernel's suspend counter (/sys/power/suspend_stats/success) to go up.
B=/sys/class/power_supply/macsmc-battery
S=/sys/power/suspend_stats
[ "$(cat $B/status)" = Discharging ] || { echo "plug OUT the charger first"; exit 1; }

ok0=$(cat $S/success); fail0=$(cat $S/fail)
e0=$(cat $B/energy_now); t0=$(date +%s)
echo "before: $(cat $B/capacity)%  energy_now=$e0 (uWh)  hook installed: $([ -x /usr/lib/systemd/system-sleep/s2idle-trim ] && echo yes || echo no)"
systemctl suspend || { echo "systemctl suspend failed"; exit 1; }

# wait for resume (or a failed suspend). The success check comes first, so after a long
# suspend we break before the 60s "never suspended" check can fire.
while :; do
    [ "$(cat $S/success)" -gt "$ok0" ] && break
    [ "$(cat $S/fail)" -gt "$fail0" ] && { echo "suspend FAILED (see journalctl -k)"; exit 1; }
    [ $(( $(date +%s) - t0 )) -gt 60 ] && { echo "never suspended within 60s"; exit 1; }
    sleep 1
done
sleep 5   # let the battery reading settle after resume

e1=$(cat $B/energy_now); t1=$(date +%s); h=$(echo "scale=3;($t1-$t0)/3600"|bc)
echo "after:  $(cat $B/capacity)%  energy_now=$e1  elapsed=${h}h"
echo "drain: $(echo "scale=2;($e0-$e1)/1000"|bc) mWh  =  $(echo "scale=1;($e0-$e1)/1000/$h"|bc) mW avg (incl. resume)"
