# Suspend drain test

Goal: measure s2idle battery drain (mW) with and without `s2idle-trim`, so we know
whether it helps. The hook unloads Wi-Fi/BT and disarms the battery/AC/USB-C wakeup sources.
Baseline expectation: ~1.3-1.5 %/hr (macOS: ~0.1-0.3 %/hr).

Two ways to get numbers:

- **`drain-test.sh`**: one deliberate, controlled run; prints the result in the terminal.
  Works with or without the hook, so it is the only way to get the baseline.
- **Hook journal logging**: once the hook is installed, *every* suspend (lid close
  included) logs battery before/after plus drain and a runtime estimate. Passive, no script needed.

## Setup (once)

- Charge to **50-90 %**. Avoid <20 % (risk of dying mid-test) and >95 % (the gauge
  tops up/recalibrates near full, so energy can even go *up* over a suspend).
- **Unplug** the charger. `drain-test.sh` refuses to run unless status is `Discharging`.
- Close heavy apps; leave things as they normally are when you close the lid.
- Same conditions for both runs: same battery range, same apps, same Wi-Fi state, same room.
- Run from a **local terminal** (not SSH).

## Run A - baseline (hook NOT installed)

```
cd ~/prog/slop/air/suspend
sudo ./s2idle-trim.sh uninstall      # if installed; restores radios + wakeups, removes hook
./s2idle-trim.sh status              # confirm "hook installed: no"
./drain-test.sh
```

It prints the "before" reading and suspends, then waits for the kernel's suspend counter
(`/sys/power/suspend_stats/success`) to tick before reading "after". Leave it **at least
1 hour**; 2-3 hours is better, since the numbers are small. The RTC cannot wake the
machine, so **wake it yourself** (open lid / press key). It then prints:

```
before: 72%  energy_now=... (uWh)  hook installed: no
after:  70%  energy_now=...  elapsed=2.013h
drain: 1350.00 mWh  =  670.6 mW avg (incl. resume)
est. suspended runtime: 46.9h (1.9 days) from now,  66.5h (2.7 days) from full
                        = 1.50 %/hr
```

Write down: elapsed hours, average mW, %/hr. No journal lines here: the hook is not installed.

## Run B - with the hook

```
sudo ./s2idle-trim.sh install        # re-run after every edit of s2idle-trim.sh
sudo ./hook-test.sh                  # pre/post without sleeping; must print PASS
```

`hook-test.sh` drops Wi-Fi/BT for a few seconds and checks that modules, the wifi netdev,
`hci0`, and the wakeup sources all come back.

Then repeat exactly as Run A:

```
./drain-test.sh
```

After waking, check the hook's view of the same sleep:

```
journalctl -t s2idle-trim -n 40
```

Expect, in order:

```
battery pre: 72% energy_now=...uWh status=Discharging
pre: saved NM_WIFI=enabled BT_SVC=active ...
pre: wakeup disabled macsmc-battery          (+ macsmc-ac, tps6598x-source-psy-*)
pre: unloaded hci_bcm4377 / brcmfmac_wcc / brcmfmac already gone ...
restore: wakeup enabled macsmc-battery       (+ the others)
restore: loaded hci_bcm4377 / brcmfmac / brcmfmac_wcc
restore: wifi + bluetooth devices are back
battery post: 70% energy_now=...uWh status=Discharging slept=2.01h drained=...mWh avg=...mW = ...%/hr; est. suspended runtime ...h (... days) from now, ...h from full
```

Its numbers should roughly match `drain-test.sh`. The hook reads a few seconds after resume,
not 5 s after the way the script does.

If Wi-Fi/BT are missing: `sudo /usr/lib/systemd/system-sleep/s2idle-trim restore`.

## Passive logging (hook installed)

Every suspend logs a `battery post:` line. Collect them across days:

```
journalctl -t s2idle-trim -g 'battery post' --no-pager
```

The post line skips the rate and says why when:

| Note in log | Cause |
|---|---|
| `charger involved: not a drain figure` | status was not `Discharging` before or after |
| `under 10 min: too noisy for a rate` | sleep was shorter than 10 minutes |
| `no measurable drain; gauge near full?` | energy did not drop (usually near 100 %) |

Long overnight lid-closes on battery are the most useful entries.

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
Comparing the counts between Run A and Run B shows whether disarming the power-supply
wakeups mattered.

## Reading results

| Result | Meaning |
|---|---|
| B much lower than A | Wi-Fi/BT and/or the power-supply wakeups were the cost; keep the hook |
| B ~ A | Drain is SoC/firmware-internal; uninstall the hook (`sudo ./s2idle-trim.sh uninstall`) |
| Both high, wake IRQs seen | Something is waking the system; investigate the IRQ |

Caveats: `energy_now` is coarse, so short runs are noisy. The average includes suspend
entry/resume overhead. Compare mW, not %. Send both results back for analysis; the macOS number
for the same hour (if you can get it) makes a good upstream Asahi report.

## Cleanup

```
sudo ./s2idle-trim.sh uninstall      # restores radios + wakeups and removes the hook
echo 0 | sudo tee /sys/power/pm_debug_messages
```
