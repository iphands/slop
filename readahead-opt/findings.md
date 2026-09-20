# noir NAS read performance: findings

Investigation of 2026-09-19 to 2026-09-20. Question: why do model loads from noir40.lan top out
around 300–350 MB/s, and is btrfs to blame?

## Summary

- **Cause:** btrfs' default readahead window (4 MB) is too small for this array. The PERC caps
  every request at 256 KB, so a 4 MB window keeps only ~16 requests in flight. A lone sequential
  reader survives that, but as soon as anything else touches the array — a second stream, a few
  random reads, a write flush — throughput collapses to ~400 MB/s. That is the ~350 MB/s seen
  during vLLM loads.
- **Fix:** raise the btrfs readahead on `/main`. **Recommended value: 65536 KB.**
- **Not the cause:** btrfs RAID (there is none), fragmentation, zstd compression, the PERC's DRAM
  cache, the disks, the network.
- **Result:** ~350 MB/s → ~760–790 MB/s under contention, ~800 MB/s alone. The array's raw
  ceiling is ~775–830 MB/s, so this is the hardware limit.

## Setup

| | |
|---|---|
| NAS | noir40.lan, Debian 13, kernel 6.12, 125 GB RAM, 8× E5-2670 cores |
| Array | PERC H710P Mini (LSI 2208, 1 GB cache), **RAID-6 of 10× Seagate ST2000NM0043** 2 TB 7.2K SAS, 64K stripe, Adaptive Read Ahead, Write Back |
| Filesystem | btrfs on the single 14.6 T virtual disk `/dev/sdb1`, `Data: single` — **btrfs does no RAID here**; `compress=zstd:3`, `commit=120` |
| Block limits | `max_hw_sectors_kb=256` (hardware cap), `nr_requests=256`, `mq-deadline` |
| Export | NFSv4.2 over RDMA (ConnectX-3, point-to-point fiber), rsize 1 MB |
| Client | this workstation, mounts under `/mnt/noir/*` |

## The two settings that matter

| Setting | Where | Default | Now |
|---|---|---|---|
| `/sys/fs/btrfs/8d042c29-f7ed-420b-9f91-bb9737a183ae/bdi/read_ahead_kb` | noir | 4096 | **65536** (live) |
| `/sys/class/bdi/0:66` and `0:67` `read_ahead_kb` | client | 128 | 32768 |

The block device's own `read_ahead_kb=128` on `sdb` is irrelevant: it only applies to raw reads
of `/dev/sdb`, not to btrfs file reads.

## Final sweep results

Overnight run, 8 hours, 21 passes, 1,380 runs, 1,216 clean (180 excluded because a write burst
hit the array during the run, 5 skipped, 0 discarded). 13–20 clean samples per cell. All medians.

Tests (fio, read-only, run locally on noir):
- **seq** — one buffered sequential stream, whole 9.5 GiB shard
- **rand** — 4 jobs of buffered 64K random reads on a 22 GB file
- **conc** — 4 sequential streams at once, 3 GB each
- **mixed** — the seq stream plus 50 IOPS of 64K O_DIRECT random reads on another file
  (stand-in for Plex / a VM); latency columns are that other IO

| Readahead (KB) | seq MB/s | conc MB/s | conc runs < 600 | mixed stream MB/s | other IO p50 | other IO IOPS |
|---|---|---|---|---|---|---|
| 4096 (default) | 761 | **405** | 14 of 14 | **419** | 14 ms | 47 |
| 8192 | 778 | 477 | 14 of 14 | 553 | 20 ms | 34 |
| 16384 | 783 | 607 | 6 of 18 | 658 | 28 ms | 24 |
| 24576 | 781 | 655 | 1 of 18 | 691 | 40 ms | 18 |
| 32768 | 774 | 649 | 4 of 20 | 711 | 53 ms | 15 |
| 40960 | 786 | 662 | 5 of 17 | 742 | 62 ms | 14 |
| 49152 | 782 | 662 | 2 of 13 | 741 | 69 ms | 12 |
| 57344 | 785 | 646 | 5 of 15 | 761 | 78 ms | 11 |
| **65536** | 788 | 654 | 3 of 17 | 763 | 83 ms | 11 |
| 73728 | 802 | 651 | 4 of 17 | 749 | 84 ms | 11 |
| 81920 | 800 | 589 | 9 of 16 | 776 | 90 ms | 10 |
| 90112 | 801 | 638 | 7 of 16 | 780 | 89 ms | 11 |
| 98304 | 801 | 640 | 7 of 15 | 776 | 90 ms | 10 |
| 106496 | 793 | 600 | 9 of 19 | 777 | 92 ms | 10 |
| 114688 | 807 | 605 | 7 of 16 | 780 | 96 ms | 10 |
| 122880 | 810 | 635 | 5 of 14 | 774 | 93 ms | 10 |
| 131072 | 810 | 616 | 8 of 17 | 780 | 86 ms | 11 |

`rand`: 414–439 IOPS at every value — readahead does not affect random reads.
Idle-array baseline for the other IO: p50 ≈ 8.5 ms, p95 ≈ 50–65 ms, p99 ≈ 180–200 ms (the tail
is most likely cold btrfs metadata lookups, not queueing).

### What it shows

1. **4096 collapses under any contention.** Alone 761 MB/s; with four parallel streams 405 MB/s
   total; with light competing random reads 419 MB/s. Every parallel run was below 600 MB/s.
   This reproduces the original complaint.
2. **The contended stream climbs steeply, then flattens.** 419 → 553 → 658 → 691 → 711 → 742 →
   763 MB/s at 65536. Doubling again to 131072 adds 2%.
3. **Parallel streams degrade from 81920 up.** 24576–73728: ~650–660 MB/s, about 1 bad run in 5.
   81920 and above: ~590–640 MB/s, almost half the runs bad. Probable cause: four streams with
   big windows compete for the same 256 queue slots.
4. **Other IO pays for the window.** Median wait 14 ms → 62 ms (40960) → 83 ms (65536) →
   ~90–96 ms (≥ 81920), and it gets a fifth of the IOPS it asked for.
5. **A lone stream gains ~2–3% at the biggest windows** (≈805 vs ≈785 MB/s). The only thing a
   very large window helps.

## Recommendation

**65536 KB.** 98% of the best contended-stream speed, the most consistent parallel loads of any
value from 57344 up, and no worse for other IO than 131072. Versus 131072 it costs nothing on a
contended load and is clearly better for parallel loads.

Balanced alternative: **40960–49152** — ~3% less contended-stream speed, equally good parallel
loads, other IO waits 17–25% less. Reasonable if the recorder, Plex and the VMs matter as much
as load times.

Do not go below ~24576 (parallel loads collapse), and do not go above ~73728 (parallel loads
degrade, nothing gained).

### Persisting it

On noir (the unit still said `KB=131072` when last checked, 2026-09-19 23:51 — verify):

```
KB=65536 /root/main-readahead.sh install
/root/main-readahead.sh show
```

`main-readahead.service` is a oneshot bound to `main.mount`, so it re-applies on boot and on any
remount of `/main`. The sysfs value otherwise resets on every mount.

On the client: `/etc/nfs.conf` (from `config/nfs.conf`) with

```
[nfsrahead]
nfs=32768
nfs4=32768
default=128
```

nfs-utils' stock udev rule (`/usr/lib/udev/rules.d/99-nfs.rules`) calls `/usr/libexec/nfsrahead`
for every new NFS bdi; with no `/etc/nfs.conf` it answers 128. There is no fstab/mount option for
this. For the current session: `sudo scripts/nfs-readahead.sh 32768`.

## Things that turned out not to matter

- **Fragmentation / compression.** `compress=zstd:3` leaves ~32k 128K compressed extents in the
  safetensors shards, but they are mostly physically contiguous: ~100–880 real seeks per shard.
  With a large window the worst shard (883 seeks) and the cleanest (171) read at the same speed.
  Defrag is not worth doing. (`filefrag` reports logical length for compressed extents, so a naive
  seek count is inflated by ~10×.) Leave compression on; `compress-force` would only hurt.
- **The PERC's 1 GB DRAM.** It is a write-back cache plus the controller's own read-ahead buffer.
  In Direct IO mode normal reads bypass it. It does not act as a read cache and should not
  influence the btrfs setting. "Adaptive Read Ahead" ≈ "Read Ahead" on this generation.
- **Stripe size.** LSI recommends ≥ 256 KB for HDD streaming on RAID-6; this VD is 64K. Changing
  it means recreating the VD — not worth it for an estimated 10–20%.
- **Network / NFS transport.** A server-cached file reads over NFS at 3.6 GB/s.
- **Going higher than 131072.** `nr_requests=256` × 256 KB = 64 MB in flight, already saturated.

## Other findings

- **Write bursts every ~2 minutes on the array.** `commit=120` batches writes from many small
  steady writers bind-mounted on `/main` (InfluxDB, immich Postgres, UniFi Mongo, Plex database,
  Home Assistant, pihole, lancache, the openmhz recorder) into one flush. About 13% of sweep runs
  were hit; they were tagged and excluded. These bursts are the main source of the "slow mode"
  seen in the early dd sweeps, where small windows were bimodal (~780 or ~450 MB/s).
- **Raw device numbers** (user-run, O_DIRECT from the start of `/dev/sdb1`): bs=1M → 249 MB/s,
  bs=64M → 775 MB/s, 4× bs=1M in parallel → ~412 MB/s total. Queue depth, not the disks.
- **openmhz (trunk-recorder) container was crash-looping** from 2026-09-19 ~13:50, restarted by
  its 2-minute watchdog ~21–27 times an hour ("Did not find trunk-recorder pid"). This predates
  all tuning and sweeps. Rebooting the rpi3 feeder helped for one cycle only. Unresolved; look at
  `podman logs openmhz` at the moment it exits. It records to `/main/docker/openmhz/data`, so its
  writes wait behind bulk reads.
- `systemd` (pid 1) showing hundreds of MiB written is an accounting effect: a reaped child's IO
  totals are added to its parent. It reflected the container restart churn, not journald.

## Corrections made along the way

- Early estimate of 4,500–7,600 seeks per shard was wrong (see fragmentation above).
- Early claim that the array needed a nearly full queue was wrong: ~60 requests in flight keep
  8 spindles busy on a quiet array. The large window buys robustness under contention, not peak
  speed.
- The first quiet-array dd sweeps (flat ~800 MB/s from 4096 to 196608) measured the wrong thing:
  one stream, no contention, warm metadata. Only the fio tests with contention separated the
  values.
- A first coarse sweep on a different file showed a rising curve (354 → 464 → 561 → 610 → 700
  MB/s for 4096 → 131072). Never fully reconciled; most likely contention/write bursts plus cold
  metadata on first reads.

## Limits of these measurements

- Test files were re-read every pass, so their btrfs metadata was always warm. A model untouched
  for days may behave somewhat worse; that would favour a larger window, not a smaller one.
- Everything was measured locally on noir. **The end-to-end NFS number with both fixes in place
  has not been measured.** The one NFS measurement (485 MB/s) was taken while the client
  readahead was still 128 KB. A real vLLM load of a cold model is the remaining test.
- Parallel-stream results are noisy run to run; conclusions there rest on medians and bad-run
  counts over ~15 samples per value.

## Files

| Path | What |
|---|---|
| `noir:/root/sweep_test.sh` | fio sweep (seq / rand / conc / mixed), logs to `/tmp/sweep_test.log` (RAM) |
| `noir:/root/main-readahead.sh` | `show` / `apply` / `install` / `uninstall` for the btrfs knob |
| `scripts/sweep_test.sh`, `scripts/main-readahead.sh` | source for the two noir scripts above |
| `scripts/nfs-readahead.sh` | set / show client NFS readahead for the running session |
| `config/nfs.conf` | client persistence (install as `/etc/nfs.conf`) |
| `config/99-z-nfs-readahead.rules` | alternative client persistence via udev (use one, not both) |
| `logs/grid_pow2_final_21passes.log` | final overnight run, 1,380 runs |
| `logs/grid_4096step8192_final.log` | earlier partial run on the 4096+8192n grid, 212 runs |

Sweep usage: `PASSES=90 MAX_HOURS=8 IDLE_WAIT_MIN=2 /root/sweep_test.sh`. Options:
`VALUES_LIST="..."`, `START/END/STEP`, `ALWAYS=4096` (injected into every grid), `TESTS="seq mixed"`,
`CONC_SIZE`, `MAX_HOURS`. It restores the starting readahead value on any exit, including Ctrl-C.
