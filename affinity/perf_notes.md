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

**Confirmed by direct measurement, and the cause is firmware — the fix is a BIOS setting and
a reboot, not a sysfs write.** This is still the largest single finding.

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

### The sysfs route is closed — the cap is in firmware (confirmed)

Writing the sysfs node **as root fails**, and the kernel says why:

```
# echo 1 | tee /sys/devices/system/cpu/cpufreq/boost
tee: /sys/devices/system/cpu/cpufreq/boost: Invalid argument

dmesg:
  cpufreq: cpufreq_boost_trigger_state: Cannot enable BOOST
  cpufreq: store_boost: Cannot enable BOOST!
```

"Invalid argument" is misleading: `store_boost()` in `drivers/cpufreq/cpufreq.c` maps *any*
driver-side failure to `-EINVAL`, so this is the driver refusing, not a rejected value.

The driver refuses because the CPU never advertised boost to the kernel:

```
grep -m1 ^flags /proc/cpuinfo | tr ' ' '\n' | grep -x cpb   ->  no match
```

`cpb` (X86_FEATURE_CPB, CPUID Fn8000_0007_EDX[9]) is **absent**. `amd_pstate` gates
`boost_supported` on that flag, returns `-ENOTSUPP`, and cpufreq reports it as `-EINVAL`.
No sysfs, module-parameter or governor change can work around a masked CPUID bit.

**Fix: BIOS.** Set **Core Performance Boost = Auto/Enabled** (AMD CBS → Core Performance
Boost, or the vendor's OC/Performance page). Check Precision Boost Overdrive while there.

Optional runtime override, if BIOS access is inconvenient. Boost is physically gated by
`MSR_K7_HWCR` (`0xC0010015`) bit 25, `CPBDIS`. Read it first — `rdmsr`/`wrmsr` are already
installed:

```bash
modprobe msr
rdmsr -a -f 25:25 0xc0010015      # 1 on every core = firmware disabled boost
```

Clearing that bit re-enables boost on the silicon even while the CPUID flag stays masked,
which is what the various "cpb enable" scripts do. It is a documented bit and the change is
lost on reboot, but it *is* a raw MSR write — your call whether that is acceptable here.
The BIOS setting is the clean fix.

Nothing in userspace was setting `boost=0`, so this is not a stray config: `/etc/local.d/`,
`/etc/sysfs.conf`, `/etc/tmpfiles.d/`, `/etc/udev/rules.d/`, `/etc/conf.d/cpupower`, tuned
and the enabled systemd units are all clean. The 0 is the driver reporting what firmware
told it.

After changing the BIOS setting, verify with the cycle counter — **not** with
`scaling_cur_freq` or `/proc/cpuinfo`, both of which lie here (Finding 2):

```bash
perf stat -e cycles -- taskset -c 3 bash -c 'end=$((SECONDS+3)); while [ $SECONDS -lt $end ]; do :; done'
```

Divide cycles by the elapsed seconds perf reports. Expect ~4.8-5.0 GHz on a lightly loaded
box; ~3.36 GHz means it is still capped. `grep -x cpb` on `/proc/cpuinfo` flags should also
start matching.

### But the monitors show 5 GHz — why that is an artifact

Observed on this box: cores appear to sit near 5 GHz much of the time and "drop under load".
That is the opposite of reality, and it is a reporting artifact:

```
at rest:   cpu9  = 5086.181     <- exactly cpuinfo_max_freq, to the digit
           others ~3362-3378    <- varying, i.e. genuine aperf/mperf samples
```

An idle core has no recent aperf/mperf delta to derive a frequency from, so the reported
value falls back to the policy maximum — a constant, always exactly `cpuinfo_max_freq`
(5086.181). As soon as the core does real work there is a real sample, and it reports the
true ~3.37 GHz. So idle reads "5 GHz" and busy reads "3.4 GHz", which mimics genuine boost
behaviour convincingly. Anything reading `/proc/cpuinfo` — xosview included — shows this.

The controlled test that separates them, pinning one core at 100%:

```
cpu21 idle:    3368.372  3368.325  3368.335  3368.443
cpu21 loaded:  3368.509  3368.502  3368.502  3368.505  3368.495
```

No change under load, and **no core anywhere on the box ever reports a value between 3500
and 5000 MHz** — there is no intermediate boost state, only nominal and the fallback
constant. A real boost would show a continuum.

Tell for the fallback: the value is *exactly* `cpuinfo_max_freq` and does not vary between
samples. Real measurements jitter in the decimals.

### Trade-off

Boost raises package power and heat — a 5950X pulls meaningfully more at 5 GHz than at 3.4.
If boost was turned off deliberately for thermals or noise, re-enabling is a real cost, not
a free win. Watch `sensors` / `nvidia-smi` under a both-users-gaming load after flipping it.

## Finding 2 — every easy frequency reading on this box is wrong, in two different ways

**`scaling_cur_freq` reports a cached policy value.**
`/sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq` and everything reading it
(`lscpu -e=MHZ`, most monitors) returned **exactly 3368.5 MHz whether the core was idle or
pinned at 100%**.

**`/proc/cpuinfo` "cpu MHz" falls back to the maximum on idle cores.** Despite `aperfmperf`
being present, an idle core with no recent aperf/mperf delta reports exactly
`cpuinfo_max_freq` (5086.181 here) rather than its real clock. This is the one that
produces a convincing false "my CPUs are at 5 GHz and drop under load" reading — see
Finding 1.

The two failures point in opposite directions, which is why they are easy to fall for: one
under-reports a boosting core, the other over-reports an idle one. Any clock claim on this
machine must come from the cycle counter:

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

## Finding 4 — `bees` dedup daemon runs unpinned at elevated priority

```
pid 3985205  root  240% CPU  nice -3  affinity 0-31
/usr/libexec/bees -c4 --strip-paths --no-timestamps /run/bees/mnt/900bb79e-...
```

Four threads, ~2.4 cores of continuous CPU, **nice -3** (higher priority than normal), free
to run on **all 32 CPUs**. It is root-owned, so `game-affinity` does not touch it — the CCD
split isolates the two users from each other but not from this.

Why it matters for the reported symptom: at nice -3 it preempts game, compositor and audio
threads wherever it lands, on both users' CCDs, while only ever accounting for ~7% of total
CPU. That is invisible on a utilisation graph and is exactly the "nothing is maxed but
everything hitches" profile. It also competes for the same btrfs metadata and device queues
the games read through.

Dedup is throughput work with no deadline, so the fix is to make it yield rather than to
starve it:

```bash
chrt -i -a -p 0 $(pgrep -x bees)       # SCHED_IDLE: runs only on genuinely idle CPU
ionice -c 3 -p $(pgrep -x bees)        # idle I/O class
renice 19 -p $(pgrep -x bees)          # undo the nice -3
```

Persist via the unit (`systemctl edit beesd@...`): `CPUSchedulingPolicy=idle`,
`IOSchedulingClass=idle`, `Nice=19`. Pinning it to a subset of cores is the blunter
alternative, but SCHED_IDLE is better here because it costs nothing when the box is quiet.

## Finding 5 — split lock mitigation: checked, not an issue

`kernel.split_lock_mitigate = 1`, which on Zen 3 traps bus locks and throttles the offender
hard — a known cause of stalls in Wine/Proton titles. **Ruled out by measurement:**
`kernel.dmesg_restrict = 0`, and `dmesg | grep -icE 'bus lock|split lock'` returns **0**.
Nothing is being throttled. Leave the sysctl alone.

## Finding 6 — Resizable BAR is off

`NVreg_EnableResizableBar: 0` (from `/proc/driver/nvidia/params`). Both cards support it.
Needs BIOS *Above 4G Decoding* + *Re-Size BAR Support*, then
`nvidia.NVreg_EnableResizableBar=1` as a module option. Helps most exactly when host→VRAM
traffic is heavy, i.e. when both users are streaming assets.

## Finding 7 — no user's PipeWire can get realtime priority

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

## Finding 8 — GPUs idle at P8; clock-ramp latency

Both cards sit at P8 with persistence mode already enabled. With PowerMizer adaptive, light
scenes drop clocks and the ramp back costs frames.

```bash
nvidia-settings -a '[gpu:0]/GPUPowerMizerMode=1'   # prefer max performance, per session
nvidia-smi -lgc <min>,<max>                        # or lock clocks outright
```

## Finding 9 — verify PCIe trains up under load

Both cards read `2.5 GT/s x8` at the time of measurement. That is **normal downtrain at P8
idle, not a finding.** Re-read mid-game:

```bash
cat /sys/bus/pci/devices/0000:0{9,a}:00.0/current_link_speed
```

Should be 16 GT/s (Gen4). Staying at Gen1/Gen2 under load would be a BIOS or slot fault and
would matter a lot. Note the PRO 6000 is a x16 card on an x8 link — that is AM4's 16 CPU
lanes bifurcated 8/8 across `00:03.1` and `00:03.2`, expected and fine for gaming.

## Finding 10 — kernel work has nowhere to live

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
