#!/bin/bash
set -e
# You know when you've been working on the runtime for hours
# and you really don't want to faff about with just getting it running using a test
# rule so you can locally experiment with changes you've made?
# That's what this is for.


COMPILER_BACKEND="cranelift"
CACHE_BACKEND="inmemory"
CONFIG_WORKING_PATH="plaid/resources/jrp_config"
SECRETS_WORKING_PATH="/tmp/empty"

# On macOS, we need to install a brew provided version of LLVM
# so that we can compile WASM binaries.
if uname | grep -q Darwin; then
  echo "macOS detected so using LLVM from Homebrew for wasm32 compatibility"
  PATH="/opt/homebrew/opt/llvm/bin:$PATH"
fi

# Built the just_run_please module
cd modules
cargo build -p just_run_please --target wasm32-unknown-unknown --release
cd ..
# Copy the compiled module to the config directory
mkdir -p compiled_modules
cp modules/target/wasm32-unknown-unknown/release/just_run_please.wasm compiled_modules/just_run_please.wasm

cd runtime
RUST_LOG=plaid=trace cargo run --bin=plaid --no-default-features --features=${COMPILER_BACKEND} -- --config ${CONFIG_WORKING_PATH} --secrets ${SECRETS_WORKING_PATH}