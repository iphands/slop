# Kernel config review — dual-seat LLM + gaming workstation

Review of `/usr/src/linux/.config` on the dual-seat workstation, 2026-08-12.
Kernel `7.1.4-gentoo`, `CONFIG_LOCALVERSION=".027-gcc-bmq"`, gcc 15.3.0, Gentoo.

## Machine baseline

| Item | Value |
|---|---|
| CPU | Ryzen 9 5950X, 16C/32T, Zen 3 (family 25 model 33) |
| L3 topology | 2× CCD, **cpu0 L3 = `0-7,16-23`**, **cpu8 L3 = `8-15,24-31`** |
| RAM | 125 GiB, **no swap device** |
| GPU 0 | RTX PRO 6000 Blackwell Workstation (GB202GL) — LLM |
| GPU 1 | RTX 4060 (AD107) — second seat |
| Display stack | NVIDIA proprietary, `nvidia-drm.modeset=1`, `CONFIG_DRM=m` |
| NIC | Mellanox ConnectX-3 (`mlx4`, 40 GbE/RoCE) + Intel I211 (`igb`) |
| Root | btrfs on NVMe, `compress-force=zstd:1`, subvol |
| Seats | `seat0`, `seat1` (see [lightdm.md](lightdm.md)) |
| Scheduler | **mainline EEVDF** (see below — *not* BMQ) |

Boot cmdline (identifiers redacted):
```
ro rootflags=subvol=rootfs audit=0 nomodeset amd_iommu=off mitigations=off
amd_pstate=active pcie_aspm=off nvidia-drm.modeset=1 video=HDMI-A-1:3840x2160R@60
netconsole=...
```

---

## Finding 1: the `-bmq` suffix is a lie — this is stock EEVDF

`LOCALVERSION` claims BMQ, but the prjc patch is not applied. Evidence:

| Check | Result |
|---|---|
| `kernel/sched/alt_core.c`, `alt_sched.h` | **absent** |
| `kernel/sched/fair.c` | present |
| `CONFIG_FAIR_GROUP_SCHED` | `=y` — SCHED_ALT removes this symbol |
| `/proc/sys/kernel/sched_yield_type` (BMQ-only sysctl) | does not exist at runtime |

The patch presumably stopped applying during the 6.17 → 7.1 bumps and the localversion
string carried forward. **Any tuning reasoning that assumes "BMQ handles this" is wrong.**

**The replacement is better than the original.** `kernel/sched/ext.c` ships in 7.1, but
`CONFIG_SCHED_CLASS_EXT` never appears in the config because:

```
kernel/Kconfig.preempt:171
  depends on BPF_SYSCALL && BPF_JIT && DEBUG_INFO_BTF
```

`BPF_JIT=n` and `DEBUG_INFO_NONE=y` block it. Enable those (Finding 2) → sched_ext becomes
available → `scx_lavd` for gaming latency, swappable at runtime with no reboot. Strictly
more flexible than a compile-time scheduler patch.

## Finding 2: `CONFIG_BPF_JIT is not set` — eBPF runs interpreted

`BPF_SYSCALL=y`, `CGROUP_BPF=y`, `NETFILTER_BPF_LINK=y`, `NET_CLS_BPF=m` are all enabled,
so systemd's cgroup device + IP filtering goes through the **BPF interpreter** on every
call. Also why the "HID-BPF support" config section is empty.

```
CONFIG_BPF_JIT=y
CONFIG_BPF_JIT_ALWAYS_ON=y
CONFIG_DEBUG_INFO_DWARF_TOOLCHAIN_DEFAULT=y   # replaces DEBUG_INFO_NONE=y
CONFIG_DEBUG_INFO_BTF=y                        # PAHOLE_VERSION=130 present, BTF will build
```

Unlocks: sched_ext, HID-BPF, bpftrace / CO-RE tooling. One change, three subsystems.

## Finding 3: no swap + no PSI = the documented livelock

Confirms and closes the loop on [oom.md](oom.md). Current state:
`CONFIG_SWAP is not set`, `CONFIG_PSI is not set`, `CONFIG_ZRAM is not set`, `Swap: 0`.

```
CONFIG_SWAP=y
CONFIG_ZSWAP=y
CONFIG_ZRAM=y
CONFIG_ZRAM_DEF_COMP_ZSTD=y
CONFIG_PSI=y
```

`PSI` is the hard prerequisite for `systemd-oomd` and pressure-mode `earlyoom`. Without it
there is no mechanism that can act before the thrash. A modest zram device (~16 GB) costs
nothing unused and converts the anon-memory cliff into a slope. Highest-value change on
the list for the LLM workload.

## Finding 4: gaming input is substantially broken

| Symbol | State | Consequence |
|---|---|---|
| `CONFIG_INPUT_JOYSTICK` | `n` | no `JOYSTICK_XPAD` → **wired Xbox pads dead** |
| `CONFIG_HID_LOGITECH` | **symbol absent** | gated on `LEDS_CLASS` + `LEDS_CLASS_MULTICOLOR` (`drivers/hid/Kconfig:652-656`), both off → no G29/G920, no `LOGIWHEELS_FF` |
| `CONFIG_HID_SONY` | `n` | DualShock / DualSense degraded |
| `CONFIG_HID_NINTENDO` | `n` | Switch Pro controller unsupported |
| `CONFIG_HID_STEAM` | `n` | Steam / Deck controller unsupported |
| `CONFIG_HID_UNIVERSAL_PIDFF` | `n` | in-tree FFB driver disabled |

**A Kconfig symbol that is entirely missing from `.config` (rather than `# ... is not set`)
means an unmet `depends on`, not a deliberate choice.** That is the tell that sent us to
`drivers/hid/Kconfig` for the Logitech case.

Force feedback is currently dead end-to-end: `HID_UNIVERSAL_PIDFF=n` *and* the out-of-tree
DKMS tree at `/usr/src/universal-pidff` has no built module —
`find /lib/modules/$(uname -r) -name '*pidff*'` returns nothing. Since 7.1 carries the
driver in-tree, the DKMS copy can likely be dropped entirely in favour of `=m`.

Already correct: `HID_PID=y`, `INPUT_FF_MEMLESS=y`, `INPUT_EVDEV=m`, `HIDRAW=y`,
`USB_HIDDEV=y`. The FF plumbing exists; only the device drivers are missing.
`INPUT_JOYDEV=n` is fine — modern SDL2/Steam use evdev, `/dev/input/js*` is legacy.

## Finding 5: `amd_iommu=off` is redundant with the config

Cmdline disables the IOMMU outright; `/sys/kernel/iommu_groups` is empty. But the config
already sets `CONFIG_IOMMU_DEFAULT_PASSTHROUGH=y` — identity-mapped DMA, no meaningful
translation overhead. `AMD_IOMMU=y` and `IRQ_REMAP=y` are compiled in and completely inert.

Dropping `amd_iommu=off` costs ~nothing at passthrough and restores DMA protection plus the
option of VFIO/GPU passthrough later.

Related: `CONFIG_VIRTUALIZATION is not set` — no KVM at all. Rules out a Windows VM for
anti-cheat titles and any VM-based testing on a 16C/128 GB box. Cheap as modules.

## Finding 6: straightforward optimizations

| Change | Rationale |
|---|---|
| `CONFIG_HZ_300` → `CONFIG_HZ_1000` | 300 Hz is the legacy 50/60 Hz compromise value. With `PREEMPT=y` and two simultaneous gaming seats, 1 ms tick beats 3.33 ms. |
| `CONFIG_X86_NATIVE_CPU=y` | `CC_HAS_MARCH_NATIVE=y`, single known machine. Currently `X86_MINIMUM_CPU_FAMILY=64` (fully generic). Gets znver3: AVX2, BMI2, SHA-NI, VAES, CLWB. |
| `CONFIG_AUDIT=n`, `CONFIG_AUDITSYSCALL=n` | Compiled in, then neutered with `audit=0` on the cmdline. Drop both and the cmdline flag. |
| `CONFIG_MEMORY_FAILURE=y` | `ACPI_APEI_GHES=y` reports hardware errors but the kernel cannot offline a poisoned page. Worth having at 128 GB. |
| drop `nomodeset` | Leftover. Harmless with the NVIDIA blob (`nvidia_drm` loaded, `modeset=1` doing the work) but blocks simpledrm handoff, leaving console on `FB_VESA`/`FB_EFI`. `CONFIG_FB_VGA16=y` is museum-grade. |

`EDAC_AMD64=n` — only matters if running ECC UDIMMs; unverified on this board.

## Finding 7: already correct, do not change

- `CONFIG_NR_CPUS=32` — exact match.
- `CONFIG_NTSYNC=y` — correct and non-obvious for Wine/Proton.
- `CONFIG_TRANSPARENT_HUGEPAGE_MADVISE` — right for LLM workloads; `ALWAYS` would hurt.
- `CONFIG_PREEMPT=y` — full preemption, right for gaming.
- `CONFIG_NUMA=n` — correct, 5950X is a single ACPI node.
- `CONFIG_IOMMU_DEFAULT_PASSTHROUGH=y` — right default for this box.
- `CONFIG_VGA_ARB_MAX_GPUS=2` — matches the two-GPU dual-seat layout.

### `CONFIG_SCHED_CLUSTER=n` is correct — leave it off

Tempting to enable for CCD awareness. **It would do nothing on Zen 3.** From
`arch/x86/kernel/smpboot.c:754-762`:

```c
const struct cpumask *cpu_coregroup_mask(int cpu)    { return cpu_llc_shared_mask(cpu); }
const struct cpumask *cpu_clustergroup_mask(int cpu) { return cpu_l2c_shared_mask(cpu); }
```

- `SCHED_MC` (already `=y`) builds its domain from the **L3/LLC** mask → **already CCD-aware**.
- `SCHED_CLUSTER` builds from the **L2** mask. Zen 3 L2 is private per core (512 KB × 16),
  so the domain would be degenerate.

Relevant to the `ccd_vllm_test` work: the CCD split is already visible to the scheduler.
Pin against the L3 masks directly —
`taskset -c 0-7,16-23` / `numactl --physcpubind`, read from
`/sys/devices/system/cpu/cpu*/cache/index3/shared_cpu_list`.

## Finding 8: possible functional surprises

- **`CONFIG_NF_TABLES is not set`** (plus `NETFILTER_ADVANCED=n`) — only legacy iptables is
  available, but `/usr/bin/nft` is installed. An nftables ruleset has no kernel side to talk
  to. Unverified whether one is actually loaded (needs root).
- **`mitigations=off`** matches `CONFIG_CPU_MITIGATIONS=n` — consistent and intentional.
  Named only because this is a genuinely multi-user box (seat0/seat1 are different people):
  `vmscape`, `spec_rstack_overflow`, `spec_store_bypass`, `spectre_v2`, `tsa` all report
  **Vulnerable**. Fine for a home workstation; a deliberate trade rather than an oversight.

## Suggested sequencing

1. `SWAP` / `ZRAM` / `PSI` — already been bitten by this ([oom.md](oom.md))
2. Gaming input drivers — `INPUT_JOYSTICK`, `LEDS_CLASS*`+`HID_LOGITECH`, `HID_SONY`,
   `HID_NINTENDO`, `HID_STEAM`, `HID_UNIVERSAL_PIDFF`
3. `BPF_JIT` + `DEBUG_INFO_BTF` — opens the sched_ext door
4. `HZ_1000`, `X86_NATIVE_CPU`, drop audit
5. Cmdline cleanup: drop `amd_iommu=off`, `nomodeset`, `audit=0`, `mitigations=off`
   (redundant with `CPU_MITIGATIONS=n`)
6. Fix or rename the `-bmq` localversion so it stops misleading future review

## Sources

- kernel: `.config`, `kernel/Kconfig.preempt` (`SCHED_CLASS_EXT`),
  `drivers/hid/Kconfig` (`HID_LOGITECH`), `arch/x86/kernel/smpboot.c`
  (`cpu_coregroup_mask`, `cpu_clustergroup_mask`)
- related notes: [oom.md](oom.md), [lightdm.md](lightdm.md), [pitfalls.md](pitfalls.md)
