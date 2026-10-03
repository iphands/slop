# OOM lockout, swap, and remote memory over RDMA

Notes from diagnosing a memory-exhaustion lockup on the workstation (`mel00`, 125 GB RAM,
40 GbE ConnectX-3 / RoCE to `noir40.lan`). Covers why the box livelocks instead of
OOM-killing, how to make it recoverable, and whether remote RAM is usable as swap.

Machine state when these notes were taken (2026-08-10, kernel `7.1.4-gentoo.027-gcc-bmq`):

| Fact | Value |
|---|---|
| `CONFIG_SWAP` | **not set** — no swap possible at all |
| `CONFIG_PSI` | not set — rules out `systemd-oomd` |
| `CONFIG_MEMCG`, `CONFIG_CGROUPS` | `=y` |
| `CONFIG_MAGIC_SYSRQ` | `=y`, `kernel.sysrq = 1` (all functions live) |
| init / cgroup | systemd, cgroup2fs (unified) |
| RDMA | `mlx4_0` ACTIVE, `rpcrdma` loaded, NFSv4.2 `proto=rdma` mounts |

---

## The core mechanism: no swap means livelock, not OOM

With `CONFIG_SWAP=n`, anonymous memory is **unevictable** — there is nowhere to put it. When
anon allocations fill RAM, the only reclaimable pool remaining is file-backed pages, which
includes the **executable text of every running process** (`sshd`, `libc`, `systemd`).

The kernel evicts those text pages, then immediately major-faults them back in when the
process next runs. Reclaim is *technically succeeding* — it frees pages on every scan — so the
OOM killer's "did we make forward progress" heuristic keeps deciding the system is healthy.
**It never fires.** The result is a machine that is alive, at ~100% iowait, making progress at
geological speed, indefinitely.

SSH fails during this not because `sshd` is dead, but because accepting a connection requires
faulting in `sshd` + PAM + `libc` text that was just evicted, queued behind thousands of other
major faults.

**Diagnostic signature:** hard lockup with memory pressure but *no* `oom-kill` /
`Out of memory` / `Killed process` lines in `journalctl -k`. Absence of OOM log lines is the
tell — it means the kernel never got to OOM, it thrashed instead.

```bash
sudo journalctl -k --list-boots | tail -5
sudo journalctl -k -b -1 | grep -iE 'oom-kill|out of memory|Killed process'
```

Consequence: a *modest amount of local, fast* swap is not optional overhead — it gives reclaim
somewhere to put anon pages instead of cannibalizing binaries. Large *remote* swap makes
lockout worse (30-second thrash becomes a 20-minute one).

---

## Fixes, ordered by leverage

### 1. sysrq escape hatch (free, works today, no reboot)

`kernel.sysrq = 1` already enables all functions. At the physical console / IPMI / serial:

```
Alt + SysRq + F      # manually invoke the OOM killer — kills the largest hog
Alt + SysRq + E      # SIGTERM everything except init, if F is not enough
```

Works *during* thrash because it is handled in kernel interrupt context — no userspace
allocation or scheduling required. Test once on a healthy box to confirm the keyboard path
works before relying on it.

### 2. Reclaim-proof memory for the login path (no reboot)

cgroup v2 `memory.min` is **hard** protection — the kernel will not reclaim below it
(contrast `memory.low`, which is best-effort).

```ini
# /etc/systemd/system/sshd.service.d/rescue.conf
[Service]
MemoryMin=64M
OOMScoreAdjust=-900
```

```ini
# /etc/systemd/system/user.slice.d/rescue.conf
[Slice]
MemoryMin=256M
```

**Both files are required.** An SSH login spawns its session scope under `user-<uid>.slice`
via `pam_systemd`, *not* under `sshd.service`. Protecting only `sshd.service` yields a
successful authentication followed by a hung shell.

`OOMScoreAdjust=-900` only biases the kernel OOM killer once it fires; it does nothing for the
thrash case. `MemoryMin` is the part that matters here.

### 3. Userspace OOM killer

- `systemd-oomd` — **not usable**, entirely PSI-driven, `CONFIG_PSI` is off.
- `earlyoom` — **not in the Gentoo tree** (checked `/usr/portage`, absent).
- `nohang` — **available in the `guru` overlay**, which is already enabled.

```bash
emerge -av sys-process/nohang
```

`nohang` polls `/proc/meminfo` rather than PSI, so it works on the current kernel, and it
`mlockall`s itself so it stays responsive while everything else thrashes. Encode known-hog
knowledge in its `--prefer` / `--avoid` regex rules.

### 4. Kernel config deltas (next rebuild)

```
CONFIG_SWAP=y          # mandatory prerequisite for everything below
CONFIG_PSI=y           # unlocks systemd-oomd + /proc/pressure/memory monitoring
CONFIG_ZRAM=y          # compressed local swap — the swap you actually want
```

`NFS_SWAP` and `ZSWAP` do not appear in the config at all right now; both are
`depends on SWAP` and Kconfig hides them until `CONFIG_SWAP=y`.

### 5. Structural fix — cap the hog

```bash
systemd-run --scope -p MemoryMax=90G -p MemorySwapMax=0 -- ./the-greedy-thing
```

The process is OOM-killed inside its own cgroup in milliseconds; the rest of the machine never
notices. For a known recurring workload this ends the problem permanently rather than merely
making it survivable.

---

## Remote RAM as swap over 40 GbE

Feasible, but it is a **capacity** tool for genuinely oversized working sets — not a fix for
lockout. Add it last, after the machine can no longer paint itself into a corner.

### Option matrix

| Path | Client config | Server config | Reclaim-safe? |
|---|---|---|---|
| Swap file on NFSv4/RDMA | `SWAP=y`, `NFS_SWAP=y` (selects `SUNRPC_SWAP`) | none — reuse export | **no annotation** |
| NVMe-oF / RDMA | `SWAP=y`, `NVME_RDMA=m` | `NVME_TARGET_RDMA=m`, `BLK_DEV_RAM=y` | **no annotation** |
| NVMe-oF / TCP | `SWAP=y`, `NVME_TCP=m` | `NVME_TARGET_TCP=m` | **yes** |
| zram (local tier-0) | `SWAP=y`, `ZRAM=y` | — | n/a |

Already present: `SUNRPC_XPRT_RDMA=m`, `NVME_FABRICS=m`, `NVME_TARGET=m`.
`TARGET_CORE`, `INFINIBAND_ISER`, `BLK_DEV_NBD` are all off — iSER and NBD are strictly worse
options anyway, ignore them.

### The memalloc annotation gotcha

Network swap's hard problem: **freeing memory requires allocating memory.** Linux solves it
with `PF_MEMALLOC` / `SOCK_MEMALLOC` emergency reserves, and only some transports opt in.
Every caller of `sk_set_memalloc()` in the tree is a **TCP socket path**:

```
drivers/block/nbd.c:1342          sk_set_memalloc(sock->sk);
drivers/nvme/host/tcp.c:1849      sk_set_memalloc(queue->sock->sk);
drivers/scsi/iscsi_tcp.c:702      sk_set_memalloc(sk);
net/sunrpc/xprtsock.c:2142        sk_set_memalloc(xs->inet);   # NFS over TCP
```

The RDMA equivalent (`net/sunrpc/xprtrdma/transport.c:723`) is a no-op stub:

```c
static int
xprt_rdma_enable_swap(struct rpc_xprt *xprt)
{
	return 0;
}
```

It returns success, so `swapon` on an RDMA mount **will silently succeed**. The only swap
awareness in the entire RDMA transport is `transport.c:231`, which sets `PF_MEMALLOC` around
*connection establishment* — not around the data path.

Not automatically fatal: RDMA does not use skbs, and `rpcrdma` preallocates its req/rep and
sendctx pools at connect time, so the reserve is arguably unnecessary. But FRWR memory-region
replenishment still allocates, and unlike the TCP paths nobody has annotated or stress-tested
this under reclaim. **Net: all RDMA swap paths are plausibly-fine-but-unproven; every
proven-safe path is TCP.**

Swap-over-NFS itself is live code in 7.1 — `nfs_swap_activate` (`fs/nfs/file.c:570`),
`nfs_swap_rw` (`fs/nfs/direct.c:155`), wired via the `swap_rw` address_space op.

### Recommendation if pursuing it

Prefer **NVMe-oF/RDMA over swap-on-NFS**: the block path is far shorter under reclaim than
dragging the NFS client state machine plus RPC scheduler into it, and `page-cluster` / I/O
scheduler tuning applies cleanly. If an NFS swapfile is used anyway, it must be **fully
preallocated with real blocks** (`dd`, not `fallocate`) — and note that a CoW/compressing
server FS (ZFS, btrfs) under a swapfile adds a second layer of unpredictability under
pressure. `nvme-tcp` is the boring safe answer: ~25% less bandwidth on ConnectX-3, but the
only data path with real reclaim protection.

Always tier, with zram in front:

```bash
swapon -p 100 /dev/zram0        # local, compressed
swapon -p 50  /dev/nvme0n1      # remote RAM
```

Server side: back the target with `brd` (pinned) or a locked tmpfs. Backing remote swap with
memory the *server* can itself swap out is a memorable way to lose an afternoon.

### Performance calibration (mlx4 / 40 GbE RoCE)

| | 4 KiB page latency |
|---|---|
| Local DRAM | ~80 ns |
| Remote RAM via RDMA, full stack | ~15–30 µs |
| NVMe SSD | ~80–100 µs |

Throughput ceiling ~3.5–4.5 GB/s (~1M pages/sec) against a 5 GB/s theoretical line rate. Good
swap device, terrible RAM substitute — fine for cold pages, awful if the working set lives there.

### Unavoidable risk

If the server reboots, the link drops, or RoCE hiccups, the client dies. Swap-in has **no error
path** — the kernel cannot handle `EIO` on a page fault, so the outcome is a hang or panic, not
graceful degradation. NFS `hard` mounts convert this into an indefinite hang, which is arguably
worse. No mitigation actually works; using remote swap means accepting that the server's uptime
is now the client's uptime.

### Tunables once running

```
vm.page-cluster = 1        # default 3 = 8-page readahead, too much for low-latency remote
vm.swappiness = 100        # swap is fast now, let it be used
```

Plus `echo none > /sys/block/<dev>/queue/scheduler` and `rotational=0`.

Current values on this box: `swappiness=60`, `page-cluster=3`, `watermark_scale_factor=10`.

---

## Related

- `frontswap` was removed from the kernel; old remote-memory projects built on it (RAMster,
  various `nvmet` frontswap hacks) are dead. `zswap` hooks the swap layer directly now.
- Academic RDMA far-memory systems (Infiniswap, Fastswap, Leap, Hermit) are all unmaintained
  research code. CXL memory pooling is the real successor but needs hardware.
