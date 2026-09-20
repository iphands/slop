# readahead-opt

Why model loads from the NAS (`noir40.lan`) topped out around 350 MB/s, and the one setting that
fixed it. Measured 2026-09-19 → 09-20 on the real hardware, not inferred from general advice.

**Full write-up: [`findings.md`](findings.md).**

## The short version

- The array is a PERC H710P RAID-6 of 10× 2 TB 7.2K SAS disks with btrfs `single` on top. The
  controller caps every request at 256 KB.
- btrfs' default readahead is 4 MB → ~16 requests in flight. A lone stream survives that
  (~760 MB/s), but under **any** contention — parallel shard loads, a few random reads from
  Plex/a VM, a write flush — it collapses to ~400 MB/s.
- Raising `/sys/fs/btrfs/<uuid>/bdi/read_ahead_kb` to **65536** holds ~760 MB/s under contention
  and ~790 MB/s alone, which is the array's hardware ceiling.
- Not the cause: fragmentation, zstd compression, the PERC's DRAM cache, the disks, the network.

| Readahead (KB) | 1 stream | 4 parallel streams | 1 stream + competing IO | other IO p50 |
|---|---|---|---|---|
| 4096 (default) | 761 MB/s | **405** | **419** | 14 ms |
| 24576 | 781 | 655 | 691 | 40 ms |
| 49152 | 782 | 662 | 741 | 69 ms |
| **65536** | 788 | 654 | 763 | 83 ms |
| 131072 | 810 | 616 (half the runs < 600) | 780 | 86 ms |

Medians over a 21-pass overnight fio sweep, 1,216 clean runs. Above ~73728 parallel loads get
worse and nothing else improves; below ~24576 parallel loads collapse.

## Layout

| Path | What |
|---|---|
| `findings.md` | setup, full 17-value table, recommendation, what didn't matter, corrections, limits |
| `scripts/sweep_test.sh` | the fio sweep: `seq` / `rand` / `conc` / `mixed` per readahead value; runs on the NAS |
| `scripts/main-readahead.sh` | `show` / `apply` / `install` / `uninstall` the btrfs knob (systemd oneshot bound to the mount) |
| `scripts/nfs-readahead.sh` | set / show NFS **client** readahead for the running session |
| `config/nfs.conf` | NFS client persistence via nfs-utils' `nfsrahead` (install as `/etc/nfs.conf`) |
| `config/99-z-nfs-readahead.rules` | alternative client persistence via udev — use one, not both |
| `logs/` | raw sweep logs (`key=value` RUN lines) the tables were built from |

## Using it

On the NAS, as root (installed there as `/root/sweep_test.sh`, `/root/main-readahead.sh`):

```bash
KB=65536 ./main-readahead.sh install      # persist: re-applied on boot and on every remount
./main-readahead.sh show

PASSES=90 MAX_HOURS=8 IDLE_WAIT_MIN=2 ./sweep_test.sh    # overnight sweep, stops itself
tail -f /tmp/sweep_test.log
```

On the NFS client:

```bash
sudo cp config/nfs.conf /etc/nfs.conf     # applies on next mount of each share
sudo scripts/nfs-readahead.sh 32768       # existing mounts, this session only
scripts/nfs-readahead.sh show
```

`sweep_test.sh` is read-only with respect to data (`fio --readonly --allow_file_create=0`
against existing files). It changes one thing — the sysfs readahead value — and restores the
starting value on any exit, including Ctrl-C and hangup. It needs `fio`, `jq`, `fincore`.
Sweep options: `VALUES_LIST`, `START`/`END`/`STEP`, `ALWAYS=4096` (injected into every grid),
`TESTS`, `CONC_SIZE`, `MAX_HOURS`; the file paths at the top are specific to this NAS.

## Traps hit while building this

- **fio doubles your readahead unless told not to.** Its default `fadvise_hint` declares
  sequential access and Linux doubles the window. Use `--fadvise_hint=0`, and `--invalidate=0`
  on any job whose target you do not want evicted from page cache.
- **`tee` + Ctrl-C can skip your cleanup trap.** Logging through `exec > >(tee …)` means SIGINT
  kills `tee` too; the next log write then kills the script with SIGPIPE before the `EXIT` trap
  restores anything. Append to the log file directly and `trap '' PIPE`.
- **`pgrep -f` / `pkill -f` match your own shell** when the pattern appears in the command line
  that runs them. Match exact argv instead.
- **`filefrag` reports logical length for compressed btrfs extents**, so counting
  "next physical < previous physical + length" inflates the seek count ~10×.
- **udev rules:** a literal `$4` inside `PROGRAM=` must be written `$$4`, and
  `99-nfs-readahead.rules` sorts *before* nfs-utils' `99-nfs.rules` (`-` < `.`), which then
  overwrites the value.
- **Same-file re-read sweeps are best-case.** Warm btrfs metadata and a quiet array made every
  value from 4 MB to 192 MB look identical (~800 MB/s). Only tests with contention separated
  them. Report minimums and bad-run counts, not just medians.

## Not done

The end-to-end number over NFS with both readahead fixes in place has not been measured — the
one NFS figure (485 MB/s) predates the client fix. A vLLM load of a cold model is the test.
