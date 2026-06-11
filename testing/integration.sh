#!/bin/bash

# Set up all the variables we need to run the integration tests
CONFIG_PATH="runtime/plaid/resources/config"
CONFIG_WORKING_PATH="/tmp/plaid_config/configs"

SECRET_PATH="runtime/plaid/resources/secrets.example.toml"
SECRET_WORKING_PATH="/tmp/plaid_config/secrets.example.toml"

export REQUEST_HANDLER=$(pwd)/runtime/target/release/request_handler


# Compiler should be passed in as the first argument
if [ -z "$1" ]; then
  echo "No compiler specified. Please specify a compiler as the first argument."
  exit 1
fi
echo "Testing runtime with compiler: $1"

# Cache backend should be passed in as the second argument
if [ -z "$2" ]; then
  echo "No cache backend specified. Defaulting to in-memory cache."
  CACHE_BACKEND="inmemory"
else
  CACHE_BACKEND="$2"
fi
echo "Testing runtime with cache backend: $CACHE_BACKEND"

# Set up the working directory
rm -rf $CONFIG_WORKING_PATH
mkdir -p $CONFIG_WORKING_PATH

# Copy the configuration and secrets to the tmp directory
cp -r $CONFIG_PATH/* $CONFIG_WORKING_PATH
cp $SECRET_PATH $SECRET_WORKING_PATH

# Use the correct config file for the chosen cache backend
mv $CONFIG_WORKING_PATH/cache.toml.$CACHE_BACKEND $CONFIG_WORKING_PATH/cache.toml

# On macOS, we need to install a brew provided version of LLVM
# so that we can compile WASM binaries.
if uname | grep -q Darwin; then
  echo "macOS detected so using LLVM from Homebrew for wasm32 compatibility"
  PATH="/opt/homebrew/opt/llvm/bin:$PATH"
fi

echo "Building All Plaid Modules"
cd modules
cargo build --all --release
cd ..

echo "Copying Compiled Test Modules to compiled_modules"
mkdir -p compiled_modules
cp -r modules/target/wasm32-unknown-unknown/release/test_*.wasm compiled_modules/

# Generate a new key without a passphrase
ssh-keygen -t ed25519 -f plaidrules_key_ed25519 -N ""
public_key=$(cat plaidrules_key_ed25519.pub | awk '{printf "%s %s %s", $1, $2, $3}')

# Generate a self-signed cert to test MNRs with
openssl genrsa -out ca.key 4096

# Generate a self-signed CA cert with CA:TRUE
openssl req -x509 -new -nodes \
  -key ca.key \
  -days 1 \
  -subj "/CN=My Test CA" \
  -addext "basicConstraints = CA:TRUE,pathlen:1" \
  -out ca.pem

# print CA cert
echo "CA Certificate"
cat ca.pem

# Generate a server key + CSR
openssl genrsa -out server.key 4096
openssl req -new -key server.key \
  -subj "/CN=localhost" \
  -out server.csr

# Create extfile for leaf cert
cat > san.cnf <<EOF
basicConstraints=CA:FALSE
subjectAltName=DNS:localhost
EOF

# Sign the server CSR with CA
openssl x509 -req \
  -in server.csr \
  -CA ca.pem -CAkey ca.key -CAcreateserial \
  -days 1 \
  -sha256 \
  -extfile san.cnf \
  -out server.pem

escaped_cert=$(
  awk 'BEGIN { ORS="\\n" }
       {
         gsub(/\\/,"\\\\");  # escape backslashes
         gsub(/&/,"\\\&");   # escape ampersands
         print
       }' ca.pem
)
rm ca.* *.csr san.cnf
mv server.pem server.key /tmp/plaid_config

# Do any needed replacements within the secrets file
if uname | grep -q Darwin; then
    # macOS (BSD sed)
    sed -i '' "s|{CI_PUBLIC_KEY_PLACEHOLDER}|$public_key|g" $SECRET_WORKING_PATH
    sed -i '' "s|{CI_SLACK_TEST_WEBHOOK}|$SLACK_TEST_WEBHOOK|g" $SECRET_WORKING_PATH
    sed -i '' "s|{CI_SLACK_TEST_BOT_TOKEN}|$SLACK_TEST_BOT_TOKEN|g" $SECRET_WORKING_PATH
    sed -i '' "s|{CI_CERTIFICATE_PLACEHOLDER}|$escaped_cert|g" $SECRET_WORKING_PATH
else
    # Linux (GNU sed)
    sed -i "s|{CI_PUBLIC_KEY_PLACEHOLDER}|$public_key|g" $SECRET_WORKING_PATH
    sed -i "s|{CI_SLACK_TEST_WEBHOOK}|$SLACK_TEST_WEBHOOK|g" $SECRET_WORKING_PATH
    sed -i "s|{CI_SLACK_TEST_BOT_TOKEN}|$SLACK_TEST_BOT_TOKEN|g" $SECRET_WORKING_PATH
    sed -i "s|{CI_CERTIFICATE_PLACEHOLDER}|$escaped_cert|g" $SECRET_WORKING_PATH
fi

# Clear out the module_signatures directory
rm -rf module_signatures/*
# Remove the old sled database if there is one (happens on repeated test runs)
rm -rf /tmp/sled

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

# If the compiler is llvm, modify the config to use the llvm backend
if [ "$1" == "llvm" ]; then

  # If macOS
  if uname | grep -q Darwin; then
    sed -i '' 's/compiler_backend = "cranelift"/compiler_backend = "llvm"/g' "${CONFIG_WORKING_PATH}/loading.toml"
    export RUSTFLAGS="-L /opt/homebrew/lib/"
    export LLVM_SYS_211_PREFIX="/opt/homebrew/Cellar/llvm@21/21.1.8"
  else
    sed -i 's/compiler_backend = "cranelift"/compiler_backend = "llvm"/g' ${CONFIG_WORKING_PATH}/loading.toml
  fi
fi

if [[ "$CACHE_BACKEND" == redis* ]]; then
  FEATURES="sled,$1,redis,aws,gcp"
else
  FEATURES="sled,$1,aws,gcp"
fi

cargo build --release --no-default-features --features $FEATURES
if [ $? -ne 0 ]; then
  echo "Failed to build Plaid with $1 compiler"
  # Exit with an error
  exit 1
fi
RUST_LOG=plaid=debug,aws_config=debug,aws_sdk_dynamodb=debug cargo run --bin=plaid --release --no-default-features --features $FEATURES -- --config ${CONFIG_WORKING_PATH} --secrets $SECRET_WORKING_PATH &
PLAID_PID=$!

# Wait for Plaid to boot. When it's ready, /ready will return HTTP 200.
# If Plaid is not ready within 120 seconds, give up and return an error.

url="http://localhost:8081/ready"
timeout=120
interval=5
deadline=$((SECONDS + timeout))

until curl -fsS "$url" >/dev/null 2>&1; do
  (( SECONDS >= deadline )) && {
    echo "Error: '$url' did not return HTTP 200 within ${timeout}s." >&2
    exit 1
  }
  sleep "$interval"
done

# If we are here, the readiness endpoint returned 200 OK. We can now proceed with our tests.
cd ..

# Set the variables the test harnesses will need
export PLAID_LOCATION="localhost:4554"

# Loop through all test modules in the test_modules directory
for module in modules/tests/*; do
  # Skip tests that are handled separately
  if [[ "$module" == "modules/tests/test_graceful_shutdown" ]]; then
    continue
  fi

  # Skip Redis tests when not using Redis
  if [[ "$CACHE_BACKEND" != redis* && "$module" == *redis* ]]; then
    echo "Skipping integration test for module $module"
    continue
  fi

  # If the module is a directory
  if [ -d "$module" ]; then
    # If the module has a harness.sh file
    if [ -f "$module/harness/harness.sh" ]; then
      echo "Running integration test for module $module"
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

# TODO: Move this logic into a dedicated script. It remains here temporarily
# because refactoring the test framework is outside the scope of the runtime
# graceful shutdown changes.
echo "Running graceful shutdown test..."

URL="test_graceful_shutdown"
FILE="received_data.$URL.txt"
LOG_COUNT=100

rm -f "$FILE"

# Start the webhook handler
$REQUEST_HANDLER > "$FILE" &
if [ $? -ne 0 ]; then
  echo "Failed to start request handler"
  rm $FILE
  exit 1
fi
RH_PID=$!

sleep 2

# Flood Plaid with logs then immediately send the SIGTERM
echo "Submitting $LOG_COUNT logs..."
for i in $(seq 1 "$LOG_COUNT"); do
  curl -d "{}" http://$PLAID_LOCATION/webhook/$URL
done

echo "Sending SIGTERM to Plaid..."
kill "$PLAID_PID"
wait "$PLAID_PID"

# Validate all expected outputs were written.
actual=$(wc -l < "$FILE")
if [ "$actual" -ne "$LOG_COUNT" ]; then
  echo "Expected $LOG_COUNT processed logs, got $actual"
  kill "$RH_PID" 2>/dev/null || true
  rm -f "$FILE"
  exit 1
fi

kill "$RH_PID"
rm "$FILE"
