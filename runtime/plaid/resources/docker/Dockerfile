FROM rust:1.96.1-slim-bookworm AS builder

ARG FEATURES=""
ENV FEATURES=$FEATURES

RUN rustup component add rustfmt
RUN mkdir /build
WORKDIR /build
COPY . .
# wget -qO - https://apt.llvm.org/llvm-snapshot.gpg.key | gpg --dearmor -o /usr/share/keyrings/llvm-archive-keyring.gpg
COPY plaid/resources/docker/llvm-snapshot.gpg /usr/share/keyrings/llvm-archive-keyring.gpg
RUN echo "deb [signed-by=/usr/share/keyrings/llvm-archive-keyring.gpg] https://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-22 main" | tee /etc/apt/sources.list.d/llvm.list > /dev/null

RUN apt update && apt install -y llvm-22-dev libpolly-22-dev build-essential libsodium-dev libssl-dev libzstd-dev libz-dev ca-certificates

# The llvm-sys build script probes for llvm-config under versioned names first
# (llvm-config-22, ...). Debian's LLVM packages only ship the plain
# `llvm-config` inside /usr/lib/llvm-22/bin (the versioned symlink lives in
# /usr/bin). Natively, probing the missing versioned path yields ENOENT and the
# script falls through to the plain name — but under buildkit's QEMU emulation
# (used for the arm64 phase of cross builds) the binfmt shim turns that failed
# exec into exit status 127 instead of ENOENT, which the build script treats as
# fatal. Providing the versioned symlink makes the first probe succeed on every
# platform, emulated or not.
RUN ln -s llvm-config /usr/lib/llvm-22/bin/llvm-config-22

RUN LLVM_SYS_221_PREFIX=/usr/lib/llvm-22 cargo build --release --bin=plaid --features=${FEATURES}

FROM ubuntu:24.04 AS runtime
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /build/target/release/plaid /plaid
RUN useradd -m plaiduser
USER plaiduser
CMD [ "/plaid", "--config", "/config/config", "--secrets", "/config/secrets.json" ]
