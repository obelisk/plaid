#!/bin/bash
set -e

# Build all of the Plaid workspace
PLATFORM=$(uname -a)

cargo build --all --release
#cargo build --package plaid --release
#cargo build --package time --target wasm32-unknown-unknown --release
#cargo build --package persistent_response --target wasm32-unknown-unknown --release

# Copy all the test modules in for loading
mkdir -p modules
cp -r target/wasm32-unknown-unknown/release/*.wasm modules/

# Run Plaid and wait for it to finish starting
RUST_LOG=plaid=debug cargo run --bin=plaid --release -- --config plaid/resources/plaid.toml --secrets plaid/resources/secrets.example.json &
PLAID_PID=$!
sleep 10

# Set the variables the test harnesses will need
export PLAID_LOCATION="localhost:4554"

# Loop through all test modules in the test_modules directory
for module in test_modules/*; do
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

# Kill the Plaid process
kill $PLAID_PID
