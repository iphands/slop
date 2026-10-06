# immich-jobrate

Live jobs/sec graph for Immich job queues, for tuning concurrency.

Immich's Jobs page shows queue depth but not throughput, and its REST `completed` count is
always 0 (BullMQ `removeOnComplete: true`). This tool reads Immich's Prometheus job counters
for exact rates and falls back to measuring backlog drain over REST.

## Enable exact counters (recommended)

On the `immich_server` container:

```yaml
environment:
  IMMICH_TELEMETRY_INCLUDE: job        # or "all"
ports:
  - "8082:8082"                        # microservices worker metrics (jobs run here)
```

Check it with `curl http://<host>:8082/metrics | grep immich_jobs_`. Counters only appear after a
job of that type has run.

## Run

```sh
cargo build --release
export IMMICH_URL=http://nas:2283 IMMICH_API_KEY=...      # admin key with queue.read
./target/release/immich-jobrate --job asset_generate_thumbnails --queue thumbnailGeneration
```

| flag | default | |
|---|---|---|
| `--url` / `IMMICH_URL` | | base URL; the metrics URL defaults to `<host>:8082/metrics` |
| `--api-key` / `IMMICH_API_KEY` | | enables queue depth, ETA, and REST drain fallback |
| `--metrics-url` / `IMMICH_METRICS_URL` | `<host>:8082/metrics` | |
| `--interval` | `1s` | poll period (`500ms`, `5s`, …) |
| `--job` | all | substring filter on metric job names (`asset_generate_thumbnails`, `asset_extract_metadata`, `smart_search`, …) |
| `--queue` | all summed | REST queue (`thumbnailGeneration`, `metadataExtraction`, `smartSearch`, `faceDetection`, …) |
| `--source` | `auto` | `auto` (metrics, else REST drain after 3 failures), `metrics`, `rest` |
| `--history` | `10m` | samples kept |

Keys: `.`/`,` (or `Tab`/`Shift-Tab`) cycle the view All → each job type → All · `a` back to All · `r` reset stats · `+`/`-` zoom graph window (30s … 30m) · `q`/Esc quit.

The graph and stats panel follow the selected view. What you cycle through depends on the source:
- **metrics** mode: job types (All = sum of every job type matching `--job`). ETA divides the `--queue` backlog by the viewed rate, so pick the job that belongs to that queue.
- **REST drain** mode: queues that have had work since start (All = sum of per-queue drains). The queue line and ETA follow the selected queue.

The table on the right is in cycle order and highlights the selection.

Graph: 5s avg (cyan) + 30s avg (yellow); min/max are over the 5s line. A rate prefixed `~` (greyed) means there isn't a full window of history yet.

## REST drain mode caveat

Without metrics, "done" = drop in waiting+active+delayed per poll, plus new failures. Anything
enqueued at the same time hides that many completions. That's fine for draining a big backlog
("Missing"/"All" run with nothing else feeding it), wrong during an active upload/library scan.
Drain is tracked per queue and then summed, so one queue filling up (e.g. thumbnails fed by metadata extraction) doesn't cancel out another draining.
A queue with nothing pending reads 0/s at once (`—` in the stats panel) instead of tailing off over the window,
and when it picks up again its rates start fresh (`~`) rather than averaging in the previous run. Same for All when every queue is empty.
Pausing/resuming a queue isn't counted as work: BullMQ moves waiting jobs into `paused`, so drain counts drops in waiting+active+delayed+paused, and skips the poll where the pause state flips.

## Tuning loop

1. Start a big run (Admin → Jobs → e.g. Generate Thumbnails → Missing).
2. Watch the `1m` rate settle.
3. Change concurrency in Admin → Jobs (the worker applies it live), press `r`, and wait ~1m.
4. Compare. Stop raising concurrency once the 1m rate stops rising, or when CPU/IO saturates.

# Remote NVENC transcoding (noir → cosmo)

The stock `immich_server` on noir uses cosmo's GPU for ffmpeg through
[ffmpeg-over-ip](https://github.com/steelbrain/ffmpeg-over-ip). File I/O is tunneled back to noir,
so cosmo needs no NFS mount or path mapping. If cosmo is unreachable, the client falls back to
noir's own CPU ffmpeg.

1. Run `./ffmpeg-over-ip/fetch.sh` to get the pinned release, sha-checked, into `ffmpeg-over-ip/bin/` (gitignored).
2. On cosmo, put `FFOIP_SECRET=<openssl rand -hex 32>` in `.env` (gitignored; the compose file
   requires it), then `docker compose up -d ffmpeg-over-ip`. It listens on `192.168.2.2:5050` and
   uses GPU `${FFOIP_GPU:-0}`.
3. On noir, copy `ffmpeg-over-ip/bin/client/ffmpeg-over-ip-client` to `/opt/ffoip/`, put the same `FFOIP_SECRET` in
   the compose `.env`, merge `ffmpeg-over-ip/noir-override.yaml` into the `immich` service, and recreate it.
4. In Immich, go to Admin → Video Transcoding and set Hardware acceleration: **NVENC**, Hardware decoding: on.

Check it: run `docker exec immich_server /opt/ffoip/ffmpeg -hide_banner -f lavfi -i testsrc2=d=3 -c:v h264_nvenc /tmp/t.mp4`
on noir. The file shows up on noir, and `docker logs ffmpeg_over_ip` on cosmo logs the argv.
Only ffmpeg goes remote; ffprobe stays local.
