# Multi-stage Dockerfile for local Plaid development.
#
# Stage 1: Build the plaid runtime (cranelift only — no LLVM needed)
# Stage 2: Build WASM rule modules
# Stage 3: Minimal runtime image

# ── Stage 1: Build plaid runtime ─────────────────────────────────────────────
ARG RUST_VERSION=1.96.1
FROM rust:${RUST_VERSION}-slim-bookworm AS runtime-builder

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
ARG RUST_VERSION=1.96.1
FROM rust:${RUST_VERSION}-slim-bookworm AS module-builder

RUN rustup target add wasm32-unknown-unknown

WORKDIR /build
COPY runtime/plaid-stl/ runtime/plaid-stl/
COPY local-dev/rules/ local-dev/rules/

# Rules import host functions (storage_*, fetch_data, ...) that only exist at
# runtime when Plaid links them into the module, so the wasm link must allow
# undefined symbols. Rust >= 1.94 turned this from a warning into a hard error.
# The env var is used instead of local-dev/rules/.cargo/config.toml because
# cargo discovers .cargo/config.toml from the working directory, not from the
# manifest path, and this build runs from /build via --manifest-path.
ENV CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C link-arg=--allow-undefined"

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

EXPOSE 8080 8081

CMD ["plaid", "--config", "/config", "--secrets", "/secrets/secrets.toml"]
