# Plaid Rule Examples

Complete, working examples that demonstrate how to write plaid rules. All
examples compile and run with the local-dev Docker setup — no manual
configuration needed.

## Quick Start

```sh
cd local-dev
docker compose up --build

# In another terminal:
curl -s -X POST http://localhost:8080/webhook/echo \
  -H "Content-Type: application/json" \
  -d '{"name": "alice", "age": 30}'
```

## Examples

| Example | Description | Key APIs | Difficulty |
|---------|-------------|----------|------------|
| [hello-world](hello-world/) | Log incoming webhook payloads | `print_debug_string` | Beginner |
| [echo-json](echo-json/) | Parse JSON, return a response | `entrypoint_with_source_and_response!`, serde | Beginner |
| [counter](counter/) | Persistent storage vs in-memory cache | `storage::insert/get`, `cache::insert/get` | Beginner |
| [cron-heartbeat](cron-heartbeat/) | Timer-triggered rule (no webhook) | `Generator::Interval`, `get_time` | Beginner |
| [webhook-router](webhook-router/) | Chain rules together via logback | `log_back`, `LogSource::Logback` | Beginner |
| [rate-limiter](rate-limiter/) | Cache-based per-key rate limiting | `cache::insert/get`, response entrypoint | Intermediate |
| [request-logger](request-logger/) | Inspect HTTP headers and metadata | `get_headers`, `get_query_params` | Intermediate |
| [todo-api](todo-api/) | Full CRUD API backed by storage | `storage::*`, `list_keys` | Intermediate |
| [http-proxy](http-proxy/) | Outbound HTTP requests | `network::make_named_request` | Advanced |
| [error-handling](error-handling/) | Production error patterns | `thiserror`, `set_error_context` | Advanced |

## Testing Each Example

### echo-json
```sh
curl -s -X POST http://localhost:8080/webhook/echo \
  -H "Content-Type: application/json" \
  -d '{"name": "alice", "age": 30}'
```

### counter
```sh
# Call multiple times to see counts increase:
curl -s -X POST http://localhost:8080/webhook/counter -d 'increment'
```

### cron-heartbeat
```sh
# Runs automatically every 30 seconds. Watch the logs:
docker compose logs -f plaid 2>&1 | grep heartbeat
```

### webhook-router
```sh
# Route a payload to the hello-world rule:
curl -s -X POST http://localhost:8080/webhook/router \
  -H "Content-Type: application/json" \
  -d '{"target": "hello_world", "body": "routed message!"}'
```

### rate-limiter
```sh
# Call 6 times — first 5 succeed, 6th is rate limited:
for i in $(seq 1 6); do
  curl -s -X POST http://localhost:8080/webhook/rate-limit \
    -H "Content-Type: application/json" \
    -d '{"key": "user-alice"}'
  echo
done
```

### request-logger
```sh
curl -s -X POST http://localhost:8080/webhook/inspect \
  -H "Content-Type: application/json" \
  -H "X-Custom-Header: my-value" \
  -H "Authorization: Bearer test-token" \
  -d '{"message": "hello"}'
```

### todo-api
```sh
# Create:
curl -s -X POST http://localhost:8080/webhook/todos \
  -H "Content-Type: application/json" \
  -d '{"action": "create", "id": "1", "title": "Buy milk", "done": false}'

# List:
curl -s -X POST http://localhost:8080/webhook/todos \
  -H "Content-Type: application/json" \
  -d '{"action": "list"}'

# Delete:
curl -s -X POST http://localhost:8080/webhook/todos \
  -H "Content-Type: application/json" \
  -d '{"action": "delete", "id": "1"}'
```

### http-proxy
```sh
# GET request to httpbin.org:
curl -s -X POST http://localhost:8080/webhook/proxy \
  -H "Content-Type: application/json" \
  -d '{"method": "get"}'

# POST request with body:
curl -s -X POST http://localhost:8080/webhook/proxy \
  -H "Content-Type: application/json" \
  -d '{"method": "post", "body": "{\"hello\": \"world\"}"}'
```

### error-handling
```sh
# Valid request:
curl -s -X POST http://localhost:8080/webhook/errors \
  -H "Content-Type: application/json" \
  -d '{"value": 42}'

# Trigger validation error:
curl -s -X POST http://localhost:8080/webhook/errors \
  -H "Content-Type: application/json" \
  -d '{"value": -1}'
```

## Anatomy of a Plaid Rule

Every plaid rule is a Rust crate that compiles to WebAssembly:

```
my-rule/
  Cargo.toml      # crate-type = ["cdylib"], depends on plaid_stl
  src/lib.rs      # rule logic with entrypoint macro
```

### 1. Cargo.toml

```toml
[package]
name = "my_rule"
version = "0.1.0"
edition = "2021"

[dependencies]
plaid_stl = { path = "../../../runtime/plaid-stl" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1"

[lib]
crate-type = ["cdylib"]
```

### 2. Entry Point

Choose the macro that matches your needs:

| Macro | Signature | Use when |
|-------|-----------|----------|
| `entrypoint!()` | `fn main(data: String) -> Result<(), i32>` | Simple processing, no source info needed |
| `entrypoint_with_source!()` | `fn main(data: String, source: LogSource) -> Result<(), i32>` | Need to know if triggered by webhook, cron, or logback |
| `entrypoint_with_source_and_response!()` | `fn main(data: String, source: LogSource) -> Result<Option<String>, i32>` | Need to return a response to the webhook caller |

### 3. Config Wiring

Rules are connected to triggers via TOML config files:

- **`webhooks.toml`** — maps URL paths to rule log types
- **`loading.toml`** — maps `.wasm` filenames to log types
- **`data.toml`** — defines cron schedules for timer-triggered rules
- **`apis.toml`** — configures outbound HTTP requests, Slack, GitHub, etc.

## Writing Your Own Rule

1. Create a new directory under `rules/` with `Cargo.toml` and `src/lib.rs`
2. Add it to the workspace in `rules/Cargo.toml`
3. Add a webhook entry in `config/webhooks.toml`
4. Add a log type override in `config/loading.toml`
5. Run `docker compose up --build` to compile and test
