# Multi-stage Dockerfile for local Plaid development.
#
# Stage 1: Build the plaid runtime (cranelift only — no LLVM needed)
# Stage 2: Build WASM rule modules
# Stage 3: Minimal runtime image

# ── Stage 1: Build plaid runtime ─────────────────────────────────────────────
FROM rust:1.88.0-slim-bookworm AS runtime-builder

RUN apt-get update && apt-get install -y \
    build-essential \
    libsodium-dev \
    libssl-dev \
    libzstd-dev \
    libz-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY runtime/ runtime/

RUN cargo build \
    --manifest-path runtime/Cargo.toml \
    --release \
    --bin=plaid \
    --no-default-features \
    --features=cranelift,sled

# ── Stage 2: Build WASM modules ─────────────────────────────────────────────
FROM rust:1.88.0-slim-bookworm AS module-builder

RUN rustup target add wasm32-unknown-unknown

WORKDIR /build
COPY runtime/plaid-stl/ runtime/plaid-stl/
COPY local-dev/rules/ local-dev/rules/

RUN cargo build \
    --manifest-path local-dev/rules/Cargo.toml \
    --release \
    --target wasm32-unknown-unknown

# ── Stage 3: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    libsodium23 \
    libssl3 \
    libzstd1 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m plaiduser

# Copy runtime binary
COPY --from=runtime-builder /build/runtime/target/release/plaid /usr/local/bin/plaid

# Copy compiled WASM modules
COPY --from=module-builder \
    /build/local-dev/rules/target/wasm32-unknown-unknown/release/*.wasm \
    /modules/

# Default config + secrets paths (overridable via docker-compose volumes)
RUN mkdir -p /config /secrets /data && chown -R plaiduser:plaiduser /data

USER plaiduser

EXPOSE 8080

CMD ["plaid", "--config", "/config", "--secrets", "/secrets/secrets.toml"]
