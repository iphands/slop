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
`failed` is monotonic (kept). Track drain **per queue** and sum: downstream queues (metadata → thumbnails/OCR/faces/smartSearch) fill while upstream drains, so diffing the summed backlog cancels real work out.

## Telemetry (Prometheus via OTel) — exact per-job counters
- Enable: `IMMICH_TELEMETRY_INCLUDE=all` or comma list `api,host,io,job,repo` (also `IMMICH_TELEMETRY_EXCLUDE`). Server container only.
- Ports: API worker `8081`, microservices worker `8082` (override: `IMMICH_API_METRICS_PORT`, `IMMICH_MICROSERVICES_METRICS_PORT`; or just map e.g. `18082:8082` when the host port is taken). **Jobs run in microservices → scrape `:8082/metrics`.** Expose the port on `immich_server`.
- `telemetry.service.ts`:
  - `JobSuccess` → counter `immich.jobs.<snake(job.name)>.<success|skipped|failed>` (only when the handler returns a JobStatus)
  - `JobError` → `...failed`
  - `JobStart`/`JobComplete` → gauge `immich.queues.<snake(queue)>.active` ±1
  - `QueueStart` → counter `immich.queues.<queue>.started`
- Prometheus name: dots→`_`, counters get `_total`, e.g. `immich_jobs_asset_generate_thumbnails_success_total`,
  plus labels like `otel_scope_name`. Counters reset when the worker restarts.
- Other groups: `host` = nestjs-otel hostMetrics (process/CPU/mem); `api` = HTTP server durations + `immich_users_total`; `repo` = histogram `immich_<class>_<method>_duration` (ms) for every repository method (DB/media/ML call latency — good for finding the slow step inside a job).
- Job names are per job (`asset_generate_thumbnails`, `asset_extract_metadata`, …), not per queue. One queue can run several job names.

## Tooling
- `immich/` (slop) = `immich-jobrate` TUI: polls `:8082/metrics` (exact), falls back to REST drain; 10s/30s/1m/5m rates (1s default poll), graph, ETA.
- Tuning knob: Admin → Jobs → concurrency per queue (system config `job.<queue>.concurrency`); the worker picks it up live (`QueueService` → `setConcurrency`).

## Remote HW transcoding (no custom image) — ffmpeg-over-ip
- Immich runs ffmpeg via `fluent-ffmpeg` (`media.repository.ts`, `ffmpeg(input,{niceness:10})`) → honors **`FFMPEG_PATH`**, else `ffmpeg` on PATH. ffprobe: `ffmpeg.ffprobe` + raw `spawn('ffprobe')` → PATH only.
- Hook: bind-mount the static `ffmpeg-over-ip-client` at e.g. `/opt/ffoip/ffmpeg` + `FFMPEG_PATH=/opt/ffoip/ffmpeg`. Leave `/usr/bin/ffmpeg` alone so `fallbackToLocal` still finds the real one. ffprobe stays local.
- Client mode comes from its own filename (contains `ffprobe` → ffprobe mode). Config env: `FFMPEG_OVER_IP_{CLIENT,SERVER}_{ADDRESS,AUTH_SECRET}` (both required for env config), `…_CLIENT_FALLBACK_TO_LOCAL`, `…_LOG`, `…_DEBUG`. Codec `rewrites` need a jsonc file.
- Server always execs the ffmpeg **next to its own binary** (ignores PATH/cwd). Bundled build = 7.1.3-Jellyfin, has `scale_cuda`, `tonemap_cuda`, `hwupload_cuda`, `{h264,hevc,av1}_nvenc`; deps are glibc only → any glibc image (`debian:*-slim`).
- Docker + nvidia: default `NVIDIA_DRIVER_CAPABILITIES` is `compute,utility` → no libnvidia-encode/nvcuvid → NVENC fails. Add `video` (env + `capabilities: [gpu, compute, video, utility]`).
- Tuning: `FFOIP_READAHEAD_BYTES` (server, default 2 MiB, max 16 MiB per file), `FFOIP_RANGE_CACHE_BYTES` (default 256 MiB, backward seeks).
- Immich side: Admin → Video Transcoding → HW accel `NVENC` + hardware decoding. No local GPU probe: device discovery is only `getVideoInterfaces()` (`/dev/dri` + mali, for VAAPI/QSV/RKMPP); NVENC just builds args. On a failed HW transcode `media.service.ts` retries with less HW accel (ends in software).
- slop setup: `immich/docker-compose.yaml` service `ffmpeg-over-ip` (cosmo), `immich/ffmpeg-over-ip/{fetch.sh,noir-override.yaml}`.
