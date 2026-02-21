# Plaid Local Development

Run Plaid locally with Docker Compose. Build and test your own rules with a
fast edit-compile-run feedback loop.

## Quick Start

```bash
cd local-dev

# 1. Set up secrets (optional — skip if you don't need external APIs)
cp secrets/secrets.toml.example secrets/secrets.toml
# Edit secrets/secrets.toml with your API keys

# 2. Build and start Plaid
docker compose up --build

# 3. Send a test webhook (in another terminal)
curl -X POST http://localhost:8080/webhook/hello \
  -H "Content-Type: application/json" \
  -d '{"message": "hello from local dev"}'
```

You should see the hello-world rule log the payload in the Docker output:

```
plaid  | [hello-world] source: webhook POST
plaid  | [hello-world] payload: {"message": "hello from local dev"}
plaid  | [hello-world] parsed JSON with 1 keys
```

## Project Structure

```
local-dev/
├── docker-compose.yml          # Orchestrates the local Plaid runtime
├── docker-compose.dev.yml      # Compose override for external modules
├── Dockerfile                  # Multi-stage: build runtime + modules
├── config/                     # Plaid configuration (TOML)
│   ├── apis.toml               # External API definitions
│   ├── cache.toml              # Cache backend (InMemory default)
│   ├── data.toml               # Data generators (cron, SQS, etc.)
│   ├── executor.toml           # Thread pool configuration
│   ├── loading.toml            # Module loading and limits
│   ├── logging.toml            # Log sinks (stdout default)
│   ├── storage.toml            # Persistent storage backend
│   └── webhooks.toml           # Webhook endpoints and routing
├── secrets/
│   ├── .gitignore              # Prevents committing real secrets
│   └── secrets.toml.example    # Template — copy to secrets.toml
├── rules/                      # Your WASM rule source code
│   ├── Cargo.toml              # Workspace — add your rules here
│   ├── .cargo/config.toml      # Sets wasm32-unknown-unknown target
│   └── hello-world/            # Example rule
│       ├── Cargo.toml
│       └── src/lib.rs
├── scripts/
│   ├── build-modules.sh        # Build modules locally (no Docker)
│   ├── test-webhook.sh         # Quick webhook test helper
│   └── watch-modules.sh        # Watch .wasm files and auto-restart
└── README.md
```

## Writing a New Rule

### 1. Create the rule crate

```bash
mkdir -p rules/my-rule/src
```

**rules/my-rule/Cargo.toml:**
```toml
[package]
name = "my_rule"
description = "What this rule does"
version = "0.1.0"
edition = "2021"

[dependencies]
plaid_stl = { path = "../../../runtime/plaid-stl" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1"

[lib]
crate-type = ["cdylib"]
```

**rules/my-rule/src/lib.rs:**
```rust
use plaid_stl::{entrypoint_with_source, messages::LogSource, plaid};

entrypoint_with_source!();

fn main(data: String, source: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Got: {data}"));
    Ok(())
}
```

### 2. Register it in the workspace

Edit `rules/Cargo.toml`:
```toml
[workspace]
members = [
    "hello-world",
    "my-rule",        # Add your rule here
]
```

### 3. Add a webhook route

Edit `config/webhooks.toml`:
```toml
[webhooks."local".webhooks."my-endpoint"]
log_type = "my_rule"
headers = []
logbacks_allowed = "Unlimited"
```

The `log_type` must match the WASM filename (underscores, not hyphens).
`my_rule.wasm` -> `log_type = "my_rule"`.

### 4. Rebuild and test

```bash
docker compose up --build
curl -X POST http://localhost:8080/webhook/my-endpoint -d 'test payload'
```

## Testing External Rules (plaid-rules, company repos)

If you develop rules in a separate repository (e.g., `plaid-rules`), you can
test them against the local Plaid runtime without copying files around.

### Option A: Volume-mount pre-compiled WASM (fastest iteration)

Build your rules locally, then mount the compiled `.wasm` files into the
running container. No Docker rebuild needed — just restart.

**1. Build your rules to WASM:**

```bash
# In your rules repo (e.g., ~/dev/plaid-rules)
cd ~/dev/plaid-rules
cargo build --release --target wasm32-unknown-unknown
```

If your rules workspace doesn't have a `.cargo/config.toml` setting the
default target, you'll need the `--target` flag every time. You can add one:

```toml
# .cargo/config.toml
[build]
target = "wasm32-unknown-unknown"
```

**2. Copy `.wasm` files into `compiled_modules/`:**

```bash
mkdir -p compiled_modules
cp ~/dev/plaid-rules/target/wasm32-unknown-unknown/release/my_rule.wasm compiled_modules/
cp ~/dev/plaid-rules/target/wasm32-unknown-unknown/release/another_rule.wasm compiled_modules/
```

Note: when you mount over `/modules`, the built-in hello-world module is
hidden. If you want both, copy it in too or use the build script.

**3. Start with the dev compose file:**

```bash
docker compose -f docker-compose.yml -f docker-compose.dev.yml up
```

This uses `docker-compose.dev.yml` which mounts `compiled_modules/` into
the container at `/modules`. No custom override file needed.

**4. Add webhook routes and log type overrides as needed:**

Edit `config/webhooks.toml` and `config/loading.toml` for your external rules,
then restart:

```bash
docker compose -f docker-compose.yml -f docker-compose.dev.yml restart
```

No `--build` needed — config and modules are volume-mounted.

**5. (Optional) Auto-restart on module changes:**

Use the watch script to automatically restart the container when `.wasm`
files in `compiled_modules/` change:

```bash
./scripts/watch-modules.sh compiled_modules
```

This uses `inotifywait` if available, otherwise falls back to polling.
Re-compile your rules and the container restarts automatically.

### Option B: Symlink rules into the workspace (rebuilds in Docker)

If you want Docker to compile your external rules, symlink them into the
`local-dev/rules/` workspace.

**1. Symlink the rule crate:**

```bash
cd local-dev/rules
ln -s /path/to/plaid-rules/rules/my-rule my-rule
```

**2. Register in the workspace and add plaid-stl path override:**

Edit `rules/Cargo.toml`:
```toml
[workspace]
members = [
    "hello-world",
    "my-rule",
]
```

Your external rule's `Cargo.toml` probably references `plaid_stl` with a
different relative path. Add a path override to fix this without modifying
the external repo:

```toml
# rules/Cargo.toml
[workspace]
members = [
    "hello-world",
    "my-rule",
]
resolver = "2"

[patch.crates-io]
plaid_stl = { path = "../../runtime/plaid-stl" }
```

Or if the external rule uses a relative path dep, the symlink may just
resolve correctly if the directory structure aligns. Test with:

```bash
cd rules && cargo check --target wasm32-unknown-unknown
```

**3. Rebuild:**

```bash
docker compose up --build
```

### Option C: Build script (recommended for regular use)

Create a small script that builds your rules and stages them for the
container:

```bash
#!/usr/bin/env bash
# dev.sh — build external rules and start Plaid
set -euo pipefail

RULES_REPO="${1:-$HOME/dev/plaid-rules}"
MODULES_DIR="$(dirname "$0")/compiled_modules"

mkdir -p "$MODULES_DIR"

# Build external rules
echo "Building rules from $RULES_REPO..."
cargo build \
    --manifest-path "$RULES_REPO/Cargo.toml" \
    --release \
    --target wasm32-unknown-unknown

# Copy compiled modules
for wasm in "$RULES_REPO/target/wasm32-unknown-unknown/release/"*.wasm; do
    [ -f "$wasm" ] || continue
    name=$(basename "$wasm")
    cp "$wasm" "$MODULES_DIR/$name"
    echo "  Staged: $name"
done

# Also build local rules
echo "Building local rules..."
./scripts/build-modules.sh

# Start Plaid with both local and external modules
docker compose up "$@"
```

Then use `docker-compose.dev.yml` to mount `compiled_modules/`:

```bash
docker compose -f docker-compose.yml -f docker-compose.dev.yml up
```

### Quick reference: iteration loop

```bash
# Edit your rule in ~/dev/plaid-rules/rules/my-rule/src/lib.rs
# Then:
cd ~/dev/plaid-rules
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/my_rule.wasm ~/dev/plaid/local-dev/compiled_modules/

# In the local-dev directory:
docker compose -f docker-compose.yml -f docker-compose.dev.yml restart  # picks up new .wasm
./scripts/test-webhook.sh my-endpoint                                    # test it

# Or use the watch script to auto-restart on changes:
./scripts/watch-modules.sh compiled_modules
```

## Adding Secrets

Secrets let your rules call external APIs (Slack, GitHub, AWS, etc.)
without hardcoding credentials in config files.

### 1. Create secrets file

```bash
cp secrets/secrets.toml.example secrets/secrets.toml
```

### 2. Add your secret values

```toml
# secrets/secrets.toml
"slack-webhook-url" = "https://hooks.slack.com/services/T.../B.../..."
"slack-bot-token" = "xoxb-1234-5678-abcdef"
```

### 3. Reference secrets in config

In `config/apis.toml`, use the `{plaid-secret{key}}` syntax:

```toml
[apis."slack"]
[apis."slack".webhooks]
alerts = "{plaid-secret{slack-webhook-url}}"
[apis."slack".bot_tokens]
mybot = "{plaid-secret{slack-bot-token}}"
```

### 4. Grant your rule access

Some APIs require explicit rule allowlisting in the config. For example,
named HTTP requests need `allowed_rules`:

```toml
[apis."general".network.web_requests."my_api_call"]
verb = "post"
uri = "https://api.example.com/endpoint"
return_body = true
return_code = true
allowed_rules = ["my_rule.wasm"]
[apis."general".network.web_requests."my_api_call".headers]
Authorization = "Bearer {plaid-secret{my-api-token}}"
```

**Secrets are never committed to git.** The `secrets/` directory has a
`.gitignore` that excludes `secrets.toml`. Only the `.example` template is
tracked.

## Available plaid-stl APIs

Rules can use these APIs from `plaid_stl`:

| Module | Functions |
|--------|-----------|
| `plaid` | `print_debug_string`, `log_back`, `get_time`, `get_secrets`, `get_headers` |
| `plaid::storage` | `insert`, `get`, `delete` (per-rule key-value store) |
| `plaid::cache` | `insert`, `get` (in-memory cache with TTL) |
| `network` | `make_named_request`, `simple_json_post_request` |
| `slack` | `post_message`, `create_channel`, `invite_to_channel`, `get_id_from_email` |
| `github` | `graphql`, `get_id_from_username`, repo/PR/team operations |
| `aws::dynamodb` | DynamoDB read/write |
| `aws::kms` | KMS encrypt/decrypt/sign |
| `aws::s3` | S3 get/put |
| `gcp::google_docs` | Google Docs/Sheets creation |
| `jira` | Issue creation and updates |
| `okta` | User/group management |
| `pagerduty` | Incident creation |
| `splunk` | Log ingestion |
| `cryptography` | AES encryption, JWT validation |

Each API must be configured in `config/apis.toml` with credentials in
`secrets/secrets.toml`. Rules that don't use external APIs (logging,
parsing, storage, cache) work out of the box.

## Entry Point Macros

Plaid provides three entry point macros depending on your use case:

```rust
// Process webhook data, no response needed
entrypoint_with_source!();
fn main(data: String, source: LogSource) -> Result<(), i32> { ... }

// Process webhook data and return a response
entrypoint_with_source_and_response!();
fn main(data: String, source: LogSource) -> Result<Option<String>, i32> { ... }

// Process binary data
entrypoint_vec_with_source!();
fn main(data: Vec<u8>, source: LogSource) -> Result<(), i32> { ... }
```

## Log Type Mapping

Plaid maps WASM module filenames to log types using this logic:

1. If `loading.toml` has a `log_type_overrides` entry for the filename, use that
2. Otherwise, take everything **before the first underscore** in the filename

Examples:

| Filename | Default log type | Override needed? |
|----------|-----------------|------------------|
| `scanner.wasm` | `scanner` | No |
| `push_patrol.wasm` | `push` | Yes — add `"push_patrol.wasm" = "push_patrol"` |
| `hello_world.wasm` | `hello` | Yes — add `"hello_world.wasm" = "hello_world"` |

If your rule name has underscores, add an explicit override in
`config/loading.toml`:

```toml
[loading.log_type_overrides]
"my_rule.wasm" = "my_rule"
```

## Running Without Docker

If you have Rust installed locally:

```bash
# Install WASM target
rustup target add wasm32-unknown-unknown

# Build modules
./scripts/build-modules.sh

# Build and run the runtime (from repo root)
cd ../runtime
RUST_LOG=plaid=debug cargo run --bin=plaid \
  --no-default-features --features=cranelift,sled \
  -- --config ../local-dev/config --secrets ../local-dev/secrets/secrets.toml
```

## Webhook Reference

| Method | Path | Description |
|--------|------|-------------|
| POST | `/webhook/hello` | Triggers `hello_world` rule |
| POST | `/webhook/default` | Triggers any rule with `log_type = "default"` |
| GET  | `/webhook/health` | Returns `ok` (healthcheck) |

Add more endpoints in `config/webhooks.toml`.

## Troubleshooting

**"No module found for log type"** — The WASM filename must match the
`log_type` in `webhooks.toml`. Check that underscores/hyphens match. If
your crate name has underscores, add a `log_type_overrides` entry in
`config/loading.toml` (see [Log Type Mapping](#log-type-mapping)).

**"API not configured"** — The rule is calling an API that isn't defined in
`config/apis.toml`. Add the API config and restart.

**"secrets file not found"** — Create `secrets/secrets.toml` (can be empty):
```bash
touch secrets/secrets.toml
```

**"missing field" parse errors** — Most HashMap/Vec config fields default to
empty, but some fields (like `computation_amount`, `memory_page_count`,
`storage_size`) are still required. If you see `missing field 'foo'`, add the
field to your config (e.g., `[loading.foo]`).

**`wasm-bindgen` link errors** — A dependency is pulling in `wasm-bindgen`,
which requires a JS host. Fix by disabling default features on the offending
crate (commonly `chrono`):
```toml
chrono = { version = "0.4", default-features = false, features = ["serde"] }
```

**Docker Compose hangs on Ctrl+C** — The `docker-compose.yml` includes
`init: true` which forwards signals properly. If using an older version,
add it to your service definition.

**Build context too large / slow builds** — Make sure `.dockerignore` exists
at the repo root and excludes `**/target/` and `.git/`. Without it, Docker
sends multi-GB Rust build artifacts as context.

**Module changes not picking up** — If modules are baked into the image,
rebuild: `docker compose up --build`. If modules are volume-mounted,
a restart is enough: `docker compose restart`.
