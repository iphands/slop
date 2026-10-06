# Immich — jobs/queues API + telemetry (server/src, main, read 2026-10)

## Queue REST API (admin; header `x-api-key: <key>`)
| Method | Path | Perm | Returns |
|---|---|---|---|
| GET | `/api/queues` | `queue.read` | `QueueResponseDto[]` |
| GET | `/api/queues/:name` | `queue.read` | `QueueResponseDto` |
| PUT | `/api/queues/:name` | `queue.update` | body `{isPaused?}` |
| GET | `/api/queues/:name/jobs` | `queue.job.read` | `[{id?, name, data, timestamp}]` |
| DELETE | `/api/queues/:name/jobs` | `queue.job.delete` | |
| GET | `/api/jobs` (legacy) | `job.read` | `{ <queueName>: {jobCounts, queueStatus:{isActive,isPaused}} }` |

`QueueResponseDto = {name, isPaused, statistics:{active, completed, failed, delayed, waiting, paused}}`.
Queue names are camelCase: `thumbnailGeneration`, `metadataExtraction`, `smartSearch`, `faceDetection`,
`facialRecognition`, `videoConversion`, `sidecar`, `library`, `ocr`, …

**`completed` is always 0**: `config.repository.ts` builds BullMQ opts with `removeOnComplete: true,
removeOnFail: false`. So REST alone cannot count throughput. Best you can do is backlog drain:
Δ(waiting+active+delayed) + Δfailed per poll, which reads low while jobs are still being enqueued.
`failed` is monotonic (kept).

## Telemetry (Prometheus via OTel) — exact per-job counters
- Enable: `IMMICH_TELEMETRY_INCLUDE=all` or comma list `api,host,io,job,repo` (also `IMMICH_TELEMETRY_EXCLUDE`). Server container only.
- Ports: API worker `8081`, microservices worker `8082`. **Jobs run in microservices → scrape `:8082/metrics`.** Expose the port on `immich_server`.
- `telemetry.service.ts`:
  - `JobSuccess` → counter `immich.jobs.<snake(job.name)>.<success|skipped|failed>` (only when the handler returns a JobStatus)
  - `JobError` → `...failed`
  - `JobStart`/`JobComplete` → gauge `immich.queues.<snake(queue)>.active` ±1
  - `QueueStart` → counter `immich.queues.<queue>.started`
- Prometheus name: dots→`_`, counters get `_total`, e.g. `immich_jobs_asset_generate_thumbnails_success_total`,
  plus labels like `otel_scope_name`. Counters reset when the worker restarts.
- Job names are per job (`asset_generate_thumbnails`, `asset_extract_metadata`, …), not per queue. One queue can run several job names.

## Tooling
- `immich/` (slop) = `immich-jobrate` TUI: polls `:8082/metrics` (exact), falls back to REST drain; 5s/30s/1m/5m rates, graph, ETA.
- Tuning knob: Admin → Jobs → concurrency per queue (system config `job.<queue>.concurrency`); the worker picks it up live (`QueueService` → `setConcurrency`).
