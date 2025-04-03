FROM rust:slim-bookworm AS builder

ARG FEATURES=""
ENV FEATURES=$FEATURES

RUN rustup component add rustfmt
RUN mkdir /build
WORKDIR /build
COPY . .
COPY plaid/resources/docker/llvm-snapshot.gpg /usr/share/keyrings/llvm-archive-keyring.gpg
RUN echo "deb [signed-by=/usr/share/keyrings/llvm-archive-keyring.gpg] https://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-18 main" | tee /etc/apt/sources.list.d/llvm.list > /dev/null

RUN apt update && apt install -y llvm-18-dev libpolly-18-dev build-essential libsodium-dev libssl-dev libzstd-dev libz-dev

RUN cargo build --release --bin=plaid --features=${FEATURES}

FROM ubuntu:24.04 AS runtime
COPY --from=builder /build/target/release/plaid /plaid
RUN useradd -m plaiduser
USER plaiduser
CMD [ "/plaid", "--config", "/config/plaid.toml", "--secrets", "/config/secrets.json" ]
