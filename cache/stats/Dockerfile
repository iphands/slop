# syntax=docker/dockerfile:1.7
#
# pkgcache-stats: nginx access-log reader + dashboard.
#
# Three stages collapse into ONE static musl binary with the frontend embedded
# via rust-embed -- no runtime dependencies, and no assumption about the working
# directory (the wart this deliberately avoids inheriting from qctrl, which
# serves `frontend/dist` from a relative path).

FROM docker.io/node:22-alpine AS frontend
WORKDIR /fe
COPY frontend/package.json frontend/package-lock.json* ./
# `npm ci` needs a lockfile; fall back to install so the image builds before
# Plan 05 has committed one.
RUN npm ci 2>/dev/null || npm install
COPY frontend/ ./
RUN npm run build

FROM docker.io/rust:1-alpine AS backend
# musl-dev for rusqlite's bundled SQLite amalgamation (it compiles C).
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
# rust-embed reads this at COMPILE time; the binary is self-contained after.
COPY --from=frontend /fe/dist frontend/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin pkgcache-stats \
 && cp target/release/pkgcache-stats /pkgcache-stats

FROM docker.io/alpine:3.20
LABEL org.opencontainers.image.title="pkgcache-stats" \
      org.opencontainers.image.description="Cache statistics dashboard for the pkgcache proxy" \
      org.opencontainers.image.source="https://github.com/iphands/slop"

# tzdata only so TZ=UTC is meaningful; all arithmetic is in epoch seconds.
RUN apk add --no-cache tzdata
COPY --from=backend /pkgcache-stats /usr/local/bin/pkgcache-stats

ENV TZ=UTC \
    PKGCACHE_LOGS=/logs \
    PKGCACHE_DATA=/data \
    PKGCACHE_CACHE=/cache \
    PKGCACHE_BIND=0.0.0.0:8081 \
    PKGCACHE_TICK_SECONDS=5 \
    PKGCACHE_LOG_RETENTION_DAYS=3 \
    PKGCACHE_DB_RETENTION_DAYS=30 \
    RUST_LOG=info

EXPOSE 8081
VOLUME /data

# Numeric, matching the proxy's arbitrary-uid model: a NAMED user would only
# exist for one uid, and ./run overrides with --user ${APP_UID}:${APP_GID}.
USER 1000:1000

# busybox wget, same convention as proxy/Dockerfile. /healthz returns 503 when
# the ingest tick has gone stale, so a dead reader fails the healthcheck even
# though the HTTP server is still up.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD wget -q -O /dev/null http://127.0.0.1:8081/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/pkgcache-stats"]
