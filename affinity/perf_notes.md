# Shared-box perf notes — dual GPU, two users, both rendering

Investigation of "when we are both rendering things slow down even though CPU and GPUs are
not maxed out". Measured 2026-08-02 on this box, not inferred from general advice.

## The machine

| | iphands | merozas |
|---|---|---|
| seat | seat0 | seat1 |
| session | labwc, Wayland | fluxbox on root-owned `X :1` (lightdm) |
| GPU | card0 `09:00.0` RTX PRO 6000 Blackwell, 96 GB | card1 `0a:00.0` RTX 4060, 8 GB |
| GPU IRQ | 86 (540M fired) | 87 (119M) |
| HDMI audio IRQ | 63 | 64 |
| CPUs (after `game-affinity`) | `0-7,16-23` = CCD0 | `8-15,24-31` = CCD1 |

- Ryzen 9 5950X, 16C/32T, 2 CCDs, 2x32 MB L3, **no NUMA nodes** (single memory controller).
- 125 GB RAM, no swap, ~100 GB in page cache.
- Kernel 7.1.4-gentoo BMQ scheduler, systemd, NVIDIA open kernel module 610.43.03.
- cmdline already carries `mitigations=off amd_iommu=off pcie_aspm=off amd_pstate=passive
  nvidia-drm.modeset=1`.

## Finding 1 — CPU boost is off; cores are hard-capped at nominal

**Confirmed by direct measurement.** This is the big one and it is nearly free to fix.

```
/sys/devices/system/cpu/cpufreq/boost          = 0
/sys/devices/system/cpu/cpufreq/policy0/boost  = 0
lscpu: "Frequency boost: disabled",  "CPU(s) scaling MHz: 66%"
```

Under 100% load on a single core:

```
perf stat -e cycles -- taskset -c 3 bash -c 'end=$((SECONDS+3)); while [ $SECONDS -lt $end ]; do :; done'
  9,695,064,256 cycles / 2.8817 s = 3.364 GHz
```

Rated boost is 5086 MHz (`cpuinfo_max_freq`/`amd_pstate_max_freq`). The core sits at nominal
~3.36 GHz — 66% of rated — *while fully loaded*, so this is not idle clock-drop.

Why it presents as "CPU is not maxed out": a game's critical path is one or two threads
(render + game thread). Capping them at 66% clock lengthens every frame while total CPU
utilisation across 32 threads stays low. The bottleneck is serial speed, not core count, and
no per-core utilisation graph will show it.

### How to fix

Runtime, instant, reversible:

```bash
echo 1 | sudo tee /sys/devices/system/cpu/cpufreq/boost
```

Verify it actually took effect — **do not trust `scaling_cur_freq` here, see Finding 2**:

```bash
perf stat -e cycles -- taskset -c 3 bash -c 'end=$((SECONDS+3)); while [ $SECONDS -lt $end ]; do :; done'
```

Divide cycles by the elapsed seconds perf reports. Expect ~4.8-5.0 GHz on a lightly loaded
box. If it still reads ~3.36 GHz, the sysfs write did nothing and the cap is in firmware —
see "If sysfs does not take" below.

Persist across reboots (systemd box, so tmpfiles is the least-moving-parts option):

```bash
# /etc/tmpfiles.d/cpu-boost.conf
w /sys/devices/system/cpu/cpufreq/boost - - - - 1
```

`systemd-tmpfiles` applies `w` (write) lines at boot. Apply without rebooting with
`sudo systemd-tmpfiles --create /etc/tmpfiles.d/cpu-boost.conf`.

Alternative if the sysfs node appears too late at boot, a udev rule fires on driver bind:

```
# /etc/udev/rules.d/99-cpu-boost.rules
SUBSYSTEM=="cpu", KERNEL=="cpu0", ATTR{../cpufreq/boost}="1"
```

### Why it is currently 0

Nothing in userspace is setting it. Checked and clean: `/etc/local.d/`, `/etc/sysfs.conf`,
`/etc/tmpfiles.d/`, `/etc/udev/rules.d/`, `/etc/conf.d/cpupower`, tuned, enabled systemd
units. So it is either a kernel/driver default on this build or was set by hand at runtime.

Evidence it is a *software* cap and the write should work: the driver still advertises the
boost range — `amd_pstate_highest_perf = 166`, `amd_pstate_max_freq = 5086181`,
`scaling_max_freq = 5086181` (unclamped). If firmware had masked CPB outright, those would
report nominal.

Counter-evidence: with the `performance` governor and an unclamped 5086 MHz policy max, the
driver is already requesting maximum perf and the silicon still refuses to exceed nominal.
That pattern is also what BIOS "Core Performance Boost = Disabled" looks like.

**Both are consistent with what was measured; the sysfs write is the cheap test that
distinguishes them.** Try it first.

### If sysfs does not take

The cap is in firmware. In BIOS, set **Core Performance Boost = Auto/Enabled** (AMD CBS →
Core Performance Boost, or the vendor's overclocking page). Related knobs worth checking
while in there: Precision Boost Overdrive, and that memory is running at its rated speed
with FCLK 1:1 (see Finding 3).

### Trade-off

Boost raises package power and heat — a 5950X pulls meaningfully more at 5 GHz than at 3.4.
If boost was turned off deliberately for thermals or noise, re-enabling is a real cost, not
a free win. Watch `sensors` / `nvidia-smi` under a both-users-gaming load after flipping it.

## Finding 2 — `scaling_cur_freq` lies on this box

`/sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq` and everything that reads it
(`lscpu -e=MHZ`, most monitors) returned **exactly 3368.5 MHz whether the core was idle or
pinned at 100%**. It is a cached policy value here, not an aperf/mperf measurement.

Any clock claim on this machine must come from the cycle counter:

```bash
perf stat -e cycles -- taskset -c <cpu> <workload>   # cycles / elapsed = real GHz
```

`turbostat` is not installed; `cpupower` is. `perf_event_paranoid=2`, which is permissive
enough for the user-space `cycles:u` counting above without root.

## Finding 3 — memory bandwidth is shared and cannot be pinned around

One memory controller feeds both CCDs and the box reports no NUMA nodes, so the CCD split
that `game-affinity` enforces buys cache isolation but **zero** DRAM-bandwidth isolation.
Two simultaneous UE workloads can saturate DRAM/Infinity Fabric long before either CPU
shows 100% — a second candidate explanation for "not maxed but slow".

Not yet measured. DIMM config needs root:

```bash
sudo dmidecode -t memory | grep -E 'Configured Memory Speed|Speed|Rank|Size'
```

If configured speed is under 3200, or FCLK is not 1:1 with memclk in BIOS, that is a real
and available fix.

## Finding 4 — `kernel.split_lock_mitigate = 1`

On Zen 3 this traps bus locks and throttles the offending task hard, a known cause of
multi-hundred-millisecond stalls in Wine/Proton titles. **Not confirmed firing here** —
`dmesg` returned nothing to a non-root read, likely `dmesg_restrict`.

```bash
sudo dmesg | grep -i 'bus lock'          # confirm before bothering
sudo sysctl -w kernel.split_lock_mitigate=0
```

## Finding 5 — Resizable BAR is off

`NVreg_EnableResizableBar: 0` (from `/proc/driver/nvidia/params`). Both cards support it.
Needs BIOS *Above 4G Decoding* + *Re-Size BAR Support*, then
`nvidia.NVreg_EnableResizableBar=1` as a module option. Helps most exactly when host→VRAM
traffic is heavy, i.e. when both users are streaming assets.

## Finding 6 — no user's PipeWire can get realtime priority

`/etc/security/limits.d/25-pw-rlimits.conf` grants `rtprio 95` / `nice -19` to `@pipewire`,
but that group is **empty**:

```
pipewire:x:509:            <- no members
realtime:x:987:iphands     <- merozas missing
audio:x:18:iphands         <- merozas missing
```

Both audio graphs therefore run at normal priority. Under load that means xruns, and an xrun
stalls the game's audio submission. Fix: `sudo gpasswd -a <user> pipewire` for both, plus
`realtime` and `audio` for merozas. Requires a re-login.

## Finding 7 — GPUs idle at P8; clock-ramp latency

Both cards sit at P8 with persistence mode already enabled. With PowerMizer adaptive, light
scenes drop clocks and the ramp back costs frames.

```bash
nvidia-settings -a '[gpu:0]/GPUPowerMizerMode=1'   # prefer max performance, per session
nvidia-smi -lgc <min>,<max>                        # or lock clocks outright
```

## Finding 8 — verify PCIe trains up under load

Both cards read `2.5 GT/s x8` at the time of measurement. That is **normal downtrain at P8
idle, not a finding.** Re-read mid-game:

```bash
cat /sys/bus/pci/devices/0000:0{9,a}:00.0/current_link_speed
```

Should be 16 GT/s (Gen4). Staying at Gen1/Gen2 under load would be a BIOS or slot fault and
would matter a lot. Note the PRO 6000 is a x16 card on an x8 link — that is AM4's 16 CPU
lanes bifurcated 8/8 across `00:03.1` and `00:03.2`, expected and fine for gaming.

## Finding 9 — kernel work has nowhere to live

With each user owning a whole CCD, every unbound kworker, softirq and btrfs endio worker
lands on *somebody's* game cores; merozas' I/O completions can run on iphands' CCD. If
cross-talk persists after the above, the option is reserving a core pair per CCD (7 cores
each) and confining the unbound workqueue mask plus default IRQ affinity there. Costs each
user a core — do not do it preemptively.

## Ruled out

- **Shared Steam library on the SATA SSD** (`/mnt/STEAMLIB` → `/dev/sdc1`, Samsung 870 EVO,
  both users running the same 39 GB Palworld install). Initially flagged as the top
  suspect. **Not a problem on this box:** blocks are deduplicated and both users read the
  same files, so the second reader is served from the ~100 GB page cache rather than the
  SATA link. Owner's call, recorded so it does not get re-raised.

## Already correct, leave alone

`mitigations=off`, `amd_iommu=off`, `pcie_aspm=off`, `performance` governor, GSP firmware
enabled, MSI enabled, THP `madvise`, `vm.max_map_count=1048576`, no swap with a large page
cache.
