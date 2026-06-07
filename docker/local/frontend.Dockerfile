# Multi-stage Cargo Build for Frontend
FROM rust:slim as builder

WORKDIR /app

# Install pkg-config and libssl-dev for native compilation
RUN apt-get update && apt-get install -y pkg-config libssl-dev ca-certificates gcc g++ && rm -rf /var/lib/apt/lists/*

COPY . .

# Build release binary using cache mounts for cargo target and registry
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/target \
    cargo build --bin frontend && \
    cp target/debug/frontend /app/frontend

# Runtime Image
FROM debian:trixie-slim as runner
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/frontend .
CMD ["./frontend"]
