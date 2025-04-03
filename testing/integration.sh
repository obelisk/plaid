#!/bin/bash

# Build all of the Plaid workspace
PLATFORM=$(uname -a)

# On macOS, we need to install a brew provided version of LLVM
# so that we can compile WASM binaries.
if uname | grep -q Darwin; then
  echo "macOS detected so using LLVM from Homebrew for wasm32 compatibility"
  PATH="/opt/homebrew/opt/llvm/bin:$PATH"
fi

echo "Building Plaid Runtime"
cd runtime
cargo build --all --release
cd ..

export REQUEST_HANDLER=$(pwd)/runtime/target/release/request_handler

echo "Building Plaid All Plaid Modules"
cd modules
cargo build --all --release
cd ..

echo "Copying Compiled Test Modules to compiled_modules"
mkdir -p compiled_modules
cp -r modules/target/wasm32-unknown-unknown/release/test_*.wasm compiled_modules/

# Generate a new key without a passphrase
ssh-keygen -t ed25519 -f plaidrules_key_ed25519 -N ""
public_key=$(cat plaidrules_key_ed25519.pub | awk '{printf "%s %s %s", $1, $2, $3}')

if uname | grep -q Darwin; then
    # macOS (BSD sed)
    sed -i '' "s|{CI_PUBLIC_KEY_PLACEHOLDER}|$public_key|g" ./runtime/plaid/resources/secrets.example.json
else
    # Linux (GNU sed)
    sed -i "s|{CI_PUBLIC_KEY_PLACEHOLDER}|$public_key|g" ./runtime/plaid/resources/secrets.example.json
fi

# Create module signatures directory
mkdir module_signatures

# Iterate over all test_*.wasm files in the target directory
for wasm_file in ./modules/target/wasm32-unknown-unknown/release/test_*.wasm; do
    # Extract the base filename (without extension)
    base_name=$(basename "$wasm_file" .wasm)

    mkdir module_signatures/"$base_name".wasm

    # Compute SHA-256 hash without a trailing newline and assign it to a variable
    shasum -a 256 "$wasm_file" | awk '{printf "%s", $1}' > "$base_name".sha256
    
    # Sign the computed hash
    ssh-keygen -Y sign -n PlaidRule -f plaidrules_key_ed25519 "$base_name.sha256"

    mv "$base_name.sha256.sig" "./module_signatures/$base_name.wasm/$base_name.wasm.sig"

    rm *.sha256
done

rm plaidrules_key_ed25519*

echo "Starting Plaid In The Background and waiting for it to boot"
cd runtime
RUST_LOG=plaid=debug cargo run --bin=plaid --release -- --config plaid/resources/plaid.toml --secrets plaid/resources/secrets.example.json &
PLAID_PID=$!
cd ..
sleep 20

# Set the variables the test harnesses will need
export PLAID_LOCATION="localhost:4554"

# Loop through all test modules in the test_modules directory
for module in modules/tests/*; do
  # If the module is a directory
  if [ -d "$module" ]; then
    # If the module has a harness.sh file
    if [ -f "$module/harness/harness.sh" ]; then
      # Run the harness.sh file
      bash $module/harness/harness.sh
      # If the harness.sh file returns an error
      if [ $? -ne 0 ]; then
       echo "Integration test failed for module $module"
        # Kill the Plaid process
        kill $PLAID_PID
        # Exit with an error
        exit 1
      fi
    fi
  fi
done

echo "Tests complete. Killing Plaid"
# Kill the Plaid process
kill $PLAID_PID
