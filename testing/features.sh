#!/bin/bash

# Build all of the Plaid workspace
PLATFORM=$(uname -a)

# On macOS, we need to install a brew provided version of LLVM
# so that we can compile WASM binaries.
if  uname | grep -q Darwin; then
  echo "macOS detected so using LLVM from Homebrew for wasm32 compatibility"
  PATH="/opt/homebrew/opt/llvm/bin:$PATH"
fi

cd runtime
echo "Building Default Plaid"

cargo build --all --release
if [ $? -ne 0 ]; then
  echo "Failed to build Plaid with support for AWS"
  # Exit with an error
  exit 1
fi

echo "Building Plaid Without AWS"
cargo build --all --release --no-default-features --features=sled,cranelift
if [ $? -ne 0 ]; then
  echo "Failed to build Plaid with support for AWS"
  # Exit with an error
  exit 1
fi