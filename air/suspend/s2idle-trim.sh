#!/bin/bash
# s2idle-trim: power down Wi-Fi + Bluetooth (BCM4387, PCIe 01:00.0/.1) around s2idle,
# and disarm power-supply wakeup sources (battery/AC/USB-C) that may cause spurious wakes.
#
#   sudo ./s2idle-trim.sh install     copy self to the systemd-sleep dir
#   sudo ./s2idle-trim.sh uninstall   remove it (and restore radios)
#   sudo ./s2idle-trim.sh restore     manually bring everything back (safe to run any time)
#   sudo ./s2idle-trim.sh status
#   (systemd calls it as: s2idle-trim pre|post suspend)
#
# Design: pre-sleep NEVER blocks suspend (always exits 0, every step has a timeout).
# Post-resume restore is idempotent and does NOT trust the saved state: it re-derives what
# should be loaded from the module lists below, retries, and escalates to a PCI rescan.
# Log: journalctl -t s2idle-trim

WIFI_MODS="brcmfmac_wcc brcmfmac"      # unload order; reload is the reverse
BT_MODS="hci_bcm4377"
EXTRA_MODS=""                           # opt-in extras, e.g. "apple_isp snd_soc_aop"
ALL_MODS="$WIFI_MODS $BT_MODS $EXTRA_MODS"

HOOK=/usr/lib/systemd/system-sleep/s2idle-trim
STATE=/run/s2idle-trim.state            # records PRE-sleep state of radios/services
LOCK=/run/s2idle-trim.lock
PCI_WIFI=0000:01:00.0
PCI_BT=0000:01:00.1
# Wakeup sources to disarm while asleep. Lid/power button (macsmc-input) stays armed.
# Side effect: plugging in a charger no longer wakes the machine.
WAKE_OFF="/sys/class/power_supply/macsmc-battery /sys/class/power_supply/macsmc-ac /sys/class/power_supply/tps6598x-source-psy-*"
WAKE_STATE=/run/s2idle-trim.wake        # paths we actually disarmed
BAT=/sys/class/power_supply/macsmc-battery
BAT_STATE=/run/s2idle-trim.battery      # "epoch energy_now(uWh) status" at pre

log() { logger -t s2idle-trim -- "$*"; echo "s2idle-trim: $*" >&2; }
loaded() { grep -q "^$1 " /proc/modules; }
t() { local s=$1; shift; timeout "$s" "$@"; }   # t <secs> cmd...

wifi_up() { for d in /sys/class/net/*; do [ -d "$d/wireless" ] && return 0; done; return 1; }
bt_up()   { [ -e /sys/class/bluetooth/hci0 ]; }
wait_for() { local i; for ((i=0;i<${2:-20};i++)); do "$1" && return 0; sleep 1; done; return 1; }

save_state() {
    {
        echo "NM_WIFI=$(t 5 nmcli radio wifi 2>/dev/null || echo unknown)"
        echo "BT_SVC=$(systemctl is-active bluetooth.service 2>/dev/null)"
        echo "RFKILL_BT=$(rfkill list bluetooth 2>/dev/null | grep -c 'Soft blocked: yes')"
        echo "RFKILL_WLAN=$(rfkill list wlan 2>/dev/null | grep -c 'Soft blocked: yes')"
    } > "$STATE"
}

wake_off() {
    local d f; : > "$WAKE_STATE"
    for d in $WAKE_OFF; do
        f=$d/power/wakeup; [ -w "$f" ] || continue
        [ "$(cat "$f")" = enabled ] || continue
        if echo disabled > "$f" 2>/dev/null; then echo "$f" >> "$WAKE_STATE"; log "pre: wakeup disabled ${d##*/}"
        else log "pre: could not disable wakeup ${d##*/}"; fi
    done
}

wake_restore() {  # re-arm what we disarmed; with no record, re-arm the whole list (boot default)
    local f list
    if [ -s "$WAKE_STATE" ]; then list=$(cat "$WAKE_STATE")
    else list=$(for d in $WAKE_OFF; do echo "$d/power/wakeup"; done); fi
    for f in $list; do
        [ -w "$f" ] || continue
        [ "$(cat "$f")" = enabled ] && continue
        echo enabled > "$f" 2>/dev/null && log "restore: wakeup enabled $(basename "$(dirname "$(dirname "$f")")")" \
            || log "restore: FAILED to re-enable wakeup $f"
    done
    rm -f "$WAKE_STATE"
}

bat_pre() {
    local e st; e=$(cat $BAT/energy_now 2>/dev/null) || return; st=$(cat $BAT/status 2>/dev/null)
    echo "$(date +%s) $e $st" > "$BAT_STATE"
    log "battery pre: $(cat $BAT/capacity)% energy_now=${e}uWh status=$st"
}

bat_post() {  # log drain over the sleep: mWh, mW, %/hr, and est. suspended runtime
    [ -r "$BAT_STATE" ] || { log "battery post: no pre record"; return; }
    local t0 e0 st0 t1 e1 st1 full
    read -r t0 e0 st0 < "$BAT_STATE"; rm -f "$BAT_STATE"
    t1=$(date +%s); e1=$(cat $BAT/energy_now 2>/dev/null) || return
    st1=$(cat $BAT/status 2>/dev/null); full=$(cat $BAT/energy_full 2>/dev/null || echo 0)
    log "battery post: $(cat $BAT/capacity)% energy_now=${e1}uWh status=$st1 $(awk -v t0="$t0" -v t1="$t1" -v e0="$e0" -v e1="$e1" -v full="$full" -v st0="$st0" -v st1="$st1" 'BEGIN {
        s = t1 - t0; mwh = (e0 - e1) / 1000
        out = sprintf("slept=%.2fh drained=%.1fmWh", s/3600, mwh)
        if (st0 != "Discharging" || st1 != "Discharging") out = out " (charger involved: not a drain figure)"
        else if (s < 600) out = out " (under 10 min: too noisy for a rate)"
        else if (mwh <= 0) out = out " (no measurable drain; gauge near full?)"
        else { mw = mwh * 3600 / s
               out = out sprintf(" avg=%.0fmW = %.2f%%/hr; est. suspended runtime %.0fh (%.1f days) from now, %.0fh from full",
                                 mw, (full > 0 ? 100 * mw * 1000 / full : 0), e1/1000/mw, e1/1000/mw/24, full/1000/mw) }
        print out }')"
}

pre() {
    bat_pre
    save_state
    wake_off
    log "pre: saved $(tr '\n' ' ' < "$STATE")"
    t 10 systemctl stop bluetooth.service 2>/dev/null
    t 5 nmcli radio wifi off 2>/dev/null
    local m
    for m in $BT_MODS $EXTRA_MODS $WIFI_MODS; do
        loaded "$m" || { log "pre: $m already gone (removed as a dependency)"; continue; }
        if t 15 modprobe -r "$m" 2>/dev/null; then log "pre: unloaded $m"
        else log "pre: could not unload $m (leaving it; suspend continues)"; fi
    done
}

load_all() {  # idempotent: load anything in the lists that is missing
    local m rc=0
    for m in $(echo $ALL_MODS | tr ' ' '\n' | tac); do
        loaded "$m" && continue
        t 30 modprobe "$m" 2>/dev/null && log "restore: loaded $m" || { log "restore: modprobe $m failed"; rc=1; }
    done
    return $rc
}

restore() {
    local try ok=0
    wake_restore    # first: cheap, and independent of the Wi-Fi retries below
    for try in 1 2 3; do
        load_all
        if wait_for wifi_up 15 && wait_for bt_up 15; then ok=1; break; fi
        log "restore: try $try — wifi_up=$(wifi_up && echo y || echo n) bt_up=$(bt_up && echo y || echo n)"
        if [ "$try" = 2 ]; then   # escalate: reset the PCI devices and rescan
            log "restore: PCI remove+rescan"
            for p in $PCI_WIFI $PCI_BT; do [ -e /sys/bus/pci/devices/$p/remove ] && echo 1 > /sys/bus/pci/devices/$p/remove; done
            sleep 1; echo 1 > /sys/bus/pci/rescan; sleep 2
        else
            for m in $ALL_MODS; do loaded "$m" && t 15 modprobe -r "$m" 2>/dev/null; done
            sleep 2
        fi
    done
    [ "$ok" = 1 ] && log "restore: wifi + bluetooth devices are back" \
                  || log "restore: GAVE UP — run: sudo $HOOK restore  (or reboot)"

    # bring services/radios back to their PRE-sleep state (default: on)
    local NM_WIFI=enabled BT_SVC=active RFKILL_BT=0 RFKILL_WLAN=0
    [ -r "$STATE" ] && . "$STATE"
    [ "$RFKILL_WLAN" = 0 ] && rfkill unblock wlan 2>/dev/null
    [ "$RFKILL_BT" = 0 ]   && rfkill unblock bluetooth 2>/dev/null
    t 10 systemctl restart NetworkManager.service 2>/dev/null   # re-adopt the new wld0
    [ "$NM_WIFI" != disabled ] && t 5 nmcli radio wifi on 2>/dev/null
    [ "$BT_SVC" != inactive ] && t 10 systemctl start bluetooth.service 2>/dev/null
    rm -f "$STATE"
    bat_post
    return $((1-ok))
}

status() {
    echo "hook installed: $([ -x $HOOK ] && echo yes || echo no)"
    for m in $ALL_MODS; do echo "  $m: $(loaded $m && echo loaded || echo NOT loaded)"; done
    echo "wifi netdev: $(wifi_up && echo yes || echo NO)   bt hci0: $(bt_up && echo yes || echo NO)"
    local d; for d in $WAKE_OFF; do [ -r "$d/power/wakeup" ] && echo "  wakeup ${d##*/}: $(cat "$d/power/wakeup")"; done
}

case "$1" in
pre|post)
    [ "$2" = suspend ] || [ "$2" = suspend-then-hibernate ] || exit 0   # ignore hibernate/hybrid
    exec 9>"$LOCK"; flock -w 60 9 || exit 0
    if [ "$1" = pre ]; then pre; else restore; fi
    exit 0 ;;                                   # never fail the sleep transition
install)
    [ "$EUID" = 0 ] || { echo "run with sudo"; exit 1; }
    install -m755 "$0" "$HOOK" && echo "installed $HOOK" && echo "test: sudo $HOOK pre suspend; sleep 3; sudo $HOOK post suspend" ;;
uninstall)
    [ "$EUID" = 0 ] || { echo "run with sudo"; exit 1; }
    restore; rm -f "$HOOK" && echo "removed $HOOK" ;;
restore) [ "$EUID" = 0 ] || { echo "run with sudo"; exit 1; }; restore ;;
status)  status ;;
*) sed -n '2,10p' "$0"; exit 1 ;;
esac
