FROM rust:1.85-slim as builder

WORKDIR /app

COPY Cargo.toml ./
COPY crates/ ./crates/

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ingest /usr/local/bin/ingest
COPY --from=builder /app/target/release/server /usr/local/bin/server

ENTRYPOINT ["server"]
