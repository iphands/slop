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
| `--interval` | `1s` | poll period (`500ms`, `2s`, …) |
| `--job` | all | substring filter on metric job names (`asset_generate_thumbnails`, `asset_extract_metadata`, `smart_search`, …) |
| `--queue` | all summed | REST queue (`thumbnailGeneration`, `metadataExtraction`, `smartSearch`, `faceDetection`, …) |
| `--source` | `auto` | `auto` (metrics, else REST drain after 3 failures), `metrics`, `rest` |
| `--history` | `10m` | samples kept |

Keys: `q`/Esc quit · `r` reset stats · `+`/`-` zoom graph window (30s … 30m).

A rate prefixed `~` (greyed) means there isn't a full window of history yet.

## REST drain mode caveat

Without metrics, "done" = drop in waiting+active+delayed per poll, plus new failures. Anything
enqueued at the same time hides that many completions. That's fine for draining a big backlog
("Missing"/"All" run with nothing else feeding it), wrong during an active upload/library scan.
Per-job rates need metrics.

## Tuning loop

1. Start a big run (Admin → Jobs → e.g. Generate Thumbnails → Missing).
2. Watch the `1m` rate settle.
3. Change concurrency in Admin → Jobs (the worker applies it live), press `r`, and wait ~1m.
4. Compare. Stop raising concurrency once the 1m rate stops rising, or when CPU/IO saturates.
