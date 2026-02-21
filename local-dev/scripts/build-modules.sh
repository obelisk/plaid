#!/usr/bin/env bash
# Build WASM modules locally (without Docker).
# Requires: rustup target add wasm32-unknown-unknown
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RULES_DIR="$SCRIPT_DIR/../rules"
OUT_DIR="$SCRIPT_DIR/../compiled_modules"

mkdir -p "$OUT_DIR"

echo "Building WASM modules..."
cargo build \
    --manifest-path "$RULES_DIR/Cargo.toml" \
    --release \
    --target wasm32-unknown-unknown

# Copy all .wasm files to output directory
for wasm in "$RULES_DIR/target/wasm32-unknown-unknown/release/"*.wasm; do
    [ -f "$wasm" ] || continue
    name=$(basename "$wasm")
    cp "$wasm" "$OUT_DIR/$name"
    echo "  $name ($(du -h "$wasm" | cut -f1))"
done

echo "Done. Modules in $OUT_DIR/"
