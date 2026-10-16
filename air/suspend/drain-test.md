# Suspend drain test

Goal: measure s2idle battery drain (mW) with and without `s2idle-trim`, so we know
whether unloading Wi-Fi/BT helps. Baseline expectation: ~1.3-1.5 %/hr (macOS: ~0.1-0.3 %/hr).

## Setup (once)

- Charge to **50-90 %**. Avoid <20 % (risk of dying mid-test) and 100 % (readings near full are flaky).
- **Unplug** the charger. `drain-test.sh` refuses to run unless status is `Discharging`.
- Close heavy apps; leave things as they normally are when you close the lid.
- Same conditions for both runs: same battery range, same apps, same Wi-Fi state, same room.
- Run from a **local terminal** (not SSH).

## Run A - baseline (hook NOT installed)

```
cd ~/prog/slop/air/suspend
./s2idle-trim.sh status              # confirm "hook installed: no"
sudo ./drain-test.sh
```

It prints the "before" reading and suspends. Leave it **at least 1 hour** (longer = more
accurate; 2-3 hours is better, since the numbers are small). The RTC cannot wake the
machine, so **wake it yourself** (open lid / press key). It then prints the delta.

Write down: elapsed hours, mWh drained, average mW.

## Run B - with the hook

```
sudo ./s2idle-trim.sh install
```

Optional sanity test first (kills network for a few seconds):

```
sudo /usr/lib/systemd/system-sleep/s2idle-trim pre suspend
sleep 3
sudo /usr/lib/systemd/system-sleep/s2idle-trim post suspend
journalctl -t s2idle-trim -n 30
./s2idle-trim.sh status              # everything loaded, wifi netdev yes, hci0 yes
```

Then repeat exactly as Run A:

```
sudo ./drain-test.sh
```

After waking, check it came back cleanly:

```
journalctl -t s2idle-trim -n 30      # expect "restore: wifi + bluetooth devices are back"
```

If Wi-Fi/BT are missing: `sudo /usr/lib/systemd/system-sleep/s2idle-trim restore`.

## Optional: wake-source check (either run)

Before suspending:

```
echo 1 | sudo tee /sys/power/pm_debug_messages
```

After a long suspend:

```
journalctl -k -b | grep -Ei 'wakeup|wake irq|PM:' | tail -30
```

Repeated wake IRQs during the suspend would explain high drain independent of Wi-Fi/BT.

## Reading results

| Result | Meaning |
|---|---|
| B much lower than A | Wi-Fi/BT card was the cost; keep the hook |
| B ~ A | Drain is SoC/firmware-internal; uninstall the hook (`sudo ./s2idle-trim.sh uninstall`) |
| Both high, wake IRQs seen | Something is waking the system; investigate the IRQ |

Caveats: `energy_now` is coarse, so short runs are noisy. The average includes suspend
entry/resume overhead. Compare mW, not %. Send both results back for analysis; the macOS number
for the same hour (if you can get it) makes a good upstream Asahi report.

## Cleanup

```
sudo ./s2idle-trim.sh uninstall      # restores radios and removes the hook
echo 0 | sudo tee /sys/power/pm_debug_messages
```
