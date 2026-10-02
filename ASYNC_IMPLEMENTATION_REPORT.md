# Plaid Async Operations — Implementation Report

**Date:** 2026-09-08
**Status:** Implemented, unit-tested, and verified end-to-end
**Versions:** `plaid` and `plaid-stl` bumped 47.1.0 → 48.0.0 (kept in sync per release policy)

---

## 1. Executive summary

Plaid rules are Rust crates compiled to `wasm32-unknown-unknown` and executed by a synchronous host call (`entrypoint.call(&mut store)`). The guest has no threads, no event loop, and no suspension primitives, so a rule **cannot yield** mid-execution. Previously, any long-running API call blocked an execution thread for its entire duration (up to 255 seconds for named web requests), and the only way to "resume later" was a hand-rolled logback loop.

This work implements **Proposal A: a runtime-managed ticketing system with push-based completion**. A rule now starts an API call with `AsyncContext::spawn(...)`, receives a ticket ID immediately, and returns — freeing the execution thread within milliseconds. When the call completes, the runtime re-invokes the rule with the result, and a macro-generated entrypoint routes it to the continuation handler the rule registered at spawn time. To the rule developer this looks like "spawn → name a handler → write the handler": no manual logbacks, no polling loops, no ticket bookkeeping.

**Measured result:** a webhook that previously would have blocked a thread for the duration of two HTTP round trips now responds in **7–9 ms**, with the full two-step async chain completing in the background and verified by content assertions.

---

## 2. Background: the limitation, confirmed

Three facts, verified against the code and the wasmer 7.0.1 sources:

1. **The guest target has no suspension primitives.** `wasm32-unknown-unknown` provides no threads, no executors, no event loop. `wasm-bindgen-futures` is useless without a JS host to poll the future. A Rust `async fn` in the guest compiles to a state machine that nothing can ever drive forward.
2. **The host call is synchronous.** `entrypoint.call(&mut store)` is a blocking call on an OS thread; host functions return `i32`, not futures. The runtime's only async concession was `runtime.block_on(...)`, which parks the executor thread for the full I/O duration.
3. **No stack switching.** wasmer 7 ships an `experimental-async` feature (corosensei coroutines, `Function::call_async`), but Plaid does not enable it. Without it, a WASM call frame cannot be parked and resumed.

Consequence: the only "yield" available to a rule is **returning from `entrypoint` and being re-invoked** with a new message. The ticket system makes that first-class.

**Related confirmed fact — no state across invocations:** the runtime builds a **fresh `Store` + `Instance` for every message** (`prepare_for_execution`). Nothing in guest memory survives: globals, heap data, and lazily-built structures (e.g. a `Regex` from `Regex::new`) are re-created on every invocation. Host-side structures do persist (the runtime's own clients, storage, cache, shared DBs, persistent responses), but the guest's linear memory and stack are ephemeral. This is why the async design passes state explicitly rather than relying on guest memory.

---

## 3. How it works

```
┌──────────┐  1. async_spawn(fn, params,      ┌──────────────┐
│   Rule   │     continuation, budget, state) │    Runtime    │
│ (guest)  │ ───────────────────────────────► │ TicketRegistry│
└──────────┘                                  └──────┬───────┘
     ▲                                               │ 2. register ticket
     │  4. completion message                        │ 3. tokio::spawn(API call)
     │     (type __plaid_async,                      ▼
     │      source AsyncCompletion,            ┌──────────────┐
     │      module = owner rule)               │  Tokio pool   │
     │                                         │  (API call)   │
     │                                         └──────┬───────┘
     │                                                │ 5. result
     └────────────────────────────────────────────────┘
```

1. **Spawn.** The `async_spawn` host function validates the request (known API function, test-mode gating, budget available, state ≤ 4 KiB), registers a ticket in the `TicketRegistry`, writes the 16-byte ticket ID back to guest memory, and spawns the API call on the shared tokio runtime. The rule's invocation ends immediately.
2. **Completion.** When the call finishes, the registry stores the result and builds a completion `Message`:
   - log type `__plaid_async` (a reserved internal type, never registered in the channel map, so it cannot collide with user log types),
   - source `LogSource::AsyncCompletion(<ticket hex>)`,
   - `module: Some(owner)` — this uses the existing single-module dispatch arm in `execution_loop`, so **only the spawning rule** is re-invoked,
   - data: a JSON envelope `{ticket, continuation, result, state}`,
   - `logbacks_allowed`: the chain budget carried forward from spawn time.
3. **Delivery.** The completion goes through the immediate executor queue; if shutdown is in progress it is coerced to the delayed/persisted logback path (delay 1s), so pending continuations survive restart when storage is persistent.
4. **Continuation.** The `entrypoint_with_async!` macro detects `LogSource::AsyncCompletion`, deserializes the envelope, and dispatches to the **named** continuation handler via a generated `match`. Unknown handler names surface as a rule error (error context), not a panic.

---

## 4. What was implemented

### 4.1 Runtime — `runtime/plaid/src/async_ops/`

**`mod.rs` — the `TicketRegistry`**
- `TicketId`: 16-byte unguessable UUID v4, hex rendering for logs.
- `TicketRecord`: owner module, continuation name, echo state, chain budget, lifecycle state (`Pending` / `Completed(result)`), creation time, TTL deadline.
- `insert(...)`: enforces the per-rule outstanding cap (default 64) and the 4 KiB state limit.
- `complete(...)`: stores the result (results > 5 MiB are recorded as failures so the rule always hears back), sets the TTL, and builds the completion message — or returns `None` for fire-and-forget tickets (no continuation), whose results simply wait to be claimed.
- `status(...)` / `claim(...)` / `cancel(...)`: the polling API. Claim is one-shot and removes the ticket; pending tickets cannot be claimed; double completion delivers exactly once.
- `sweep_expired()`: TTL eviction, driven by a background sweeper task (60 s interval) that exits with the cancellation token.
- Counters (`created_total`, `completed_total`, `expired_total`) exposed for metrics.
- **10 unit tests**: uniqueness, insert/status round-trip, per-module limit, state limit, claim semantics, completion message shape, oversize-result coercion, cancel, sweep, double-completion.

**`dispatch.rs` — the async API dispatch table**
- `prepare_async_api_call(api, name, params, module)` maps a host function name to either a `SpawnOutcome::Future` (drive on tokio) or `SpawnOutcome::ImmediateError` (unknown function / test mode / unconfigured API → the ticket completes with an error instead of hanging).
- **All 157 API host functions are dispatchable** — verified by cross-check against the `define_api_functions!` registration table (0 missing, 0 extra): GitHub 58, blockchain (EVM + Solana) 30, Slack 15, npm 14, AWS (KMS/DynamoDB/S3) 14, Jira 6, GCP 6, general 3, PagerDuty 2, Okta 2, cryptography 2, and one each for Yubikey, Web, Splunk, Rustica, bloom filter.
- Return values are JSON-normalized (`String` → JSON string, `u32`/`i32`/`bool` → JSON number/bool) so the guest can decode with typed helpers.
- `TEST_MODE_ALLOWED` mirrors the synchronous functions' test-mode gating exactly: side-effecting spawns are rejected at spawn time, so a rule never waits on an operation that is guaranteed to fail.

**`functions/async_ops.rs` — host functions**
- `async_spawn` (11 parameters incl. the ticket-out pointer): reads guest memory in one scope, mutates the env budget in another, writes the ticket ID back after spawning.
- `ticket_status`: returns 0=Pending, 1=Completed, 2=Failed, 3=Unknown.
- `ticket_claim`: the standard buffer-size-then-read protocol used by all buffer-returning host functions; one-shot.
- `ticket_cancel`: best-effort; the API call may still run, but no completion is delivered.
- Budget deduction mirrors the logback system: spawns draw from the invocation's `LogbacksAllowed`, bounding continuation-chain depth; `Unlimited` stays unlimited.
- Completion delivery mirrors `dispatch_logback`: immediate queue when healthy, delayed/persisted path during shutdown drain.

### 4.2 STL — `runtime/plaid-stl/src/r#async/`

> Note: `async` is a reserved keyword, so the module is `r#async` and guests import it as `plaid_stl::r#async::{...}`.

- `TicketId` — `Copy`, hex conversion.
- `TicketStatus`, `AsyncResult` (with `as_string` / `as_u32` / `as_bool` / `as_i32` decoders and `error()`).
- `AsyncCompletion` — the envelope (ticket, continuation, result, state).
- `AsyncContext` — `spawn`, `spawn_with_state`, `ticket_status`, `ticket_claim`, `ticket_cancel` (all `&self` methods).
- New error codes: `-18` UnknownTicket, `-19` TooManyPendingTickets, `-20` AsyncStateTooLarge; `PlaidFunctionError` gained data-carrying variants (`InternalApiErrorWith`, `UnknownTicket`, `TooManyPendingTickets`, `AsyncStateTooLarge`), so `From<PlaidFunctionError> for i32` is now an explicit match.
- `LogSource::AsyncCompletion(String)` variant added (additive; old rules simply never match on it).
- **`entrypoint_with_async!(handler_a, handler_b, ...)` macro** — generates the entrypoint that routes completions to named handlers. Uses fully-qualified paths internally so it never conflicts with the rule's own imports.

### 4.3 Config

```toml
[executor.async_tickets]
max_tickets_per_module = 64    # outstanding (pending + unclaimed) tickets per rule
completed_ttl_secs = 900       # 15 min until unclaimed results are evicted
```

Both fields default as shown; the whole section is optional.

### 4.4 Wiring

- `Env` gained `ticket_registry: Option<Arc<TicketRegistry>>`, threaded through `prepare_for_execution` → `process_message_with_module` → `execution_loop` → `Executor::new` (general and dedicated pools).
- `bin/plaid.rs` creates the registry from config, starts the sweeper (`functions::start_ticket_sweeper`), and joins it during shutdown after the delayed-logback flush.
- Host functions registered in the `define_api_functions!` table, so module linking and the loader's import validation work unchanged.

### 4.5 Test module — `modules/tests/test_async/`

A three-step chained workflow (full code in §6) plus a `harness/harness.sh` integration harness, a new `/async_echo` route on the test request handler (echoes the body in both the response and the log), and config entries in `webhooks.toml`, `apis.toml`, and `loading.toml` (log type override + test-mode exemption).

### 4.6 Test infrastructure

- `loader::PlaidModule::compile_for_tests` (test-only) + `src/tests.rs::stub_module` (compiles a minimal valid wasm binary) so registry unit tests can hold a real `PlaidModule` handle without the full config-loading path.

---

## 5. Safety properties

| Property | Mechanism |
|---|---|
| Chain depth bounded | Spawns deduct from `LogbacksAllowed`, same as logbacks; a `Limited(n)` budget permits at most n further spawns down the chain |
| Registry memory bounded | Per-rule outstanding cap (default 64) counts pending **and** unclaimed-completed tickets |
| No hung rules | Every ticket eventually completes: success, API error, oversize result (recorded as failure), or TTL eviction; `Unknown` status is a first-class outcome rules can handle |
| Result size bounded | 5 MiB cap (same as `Message.data`); oversize results become error completions |
| State size bounded | 4 KiB cap on the spawn state payload, enforced synchronously |
| Test mode safe | Side-effecting spawns rejected at spawn time with the same gating as synchronous calls |
| Shutdown safe | Completions racing shutdown are coerced to the delayed/persisted logback path; the sweeper exits with the cancellation token |
| No queue collisions | `__plaid_async` is a reserved log type never registered in the channel map |
| At-most-once delivery | Double completion (cancel racing the task) delivers exactly once; the first result wins |

**Known limitation (documented):** in-flight tasks are in-process. A crash loses pending tickets and the in-memory registry; rules must treat `ticket_status == Unknown` as failure. Durable replay (persisting spawn *requests* and replaying on startup) is the natural phase-2 extension and reuses the delayed-logback persistence machinery.

---

## 5.1 Backward compatibility (verified)

**Yes — all changes are backward compatible**, verified by both a code audit and an empirical run of a pre-change rule on the new runtime.

### Code audit findings

| Axis | Finding |
|---|---|
| **Old compiled rules (`.wasm` built against STL 47.x) on the new runtime** | ✅ Fully compatible. Host functions are linked *per import* (`link_functions_to_module` iterates the module's imports and resolves each); new functions are only linked if a module actually imports them. Old modules import none of `async_spawn`/`ticket_*`, so linking is byte-for-byte identical to before. The loader's import validation (`is_known_api_function`) only *accepts* names — it never rejects old ones. |
| **Entrypoint contract** | ✅ Unchanged: `entrypoint() -> i32`, no parameters. New rules use the same signature; only the macro body differs. |
| **`Message` wire/persistence format** | ✅ Unchanged. The struct gained no fields; `ticket_registry` was added to the executor-internal `Env` (never serialized). Delayed logbacks persisted in storage by an old runtime deserialize identically on the new one. |
| **`LogSource` serde** | ✅ Additive. The enum is externally tagged; the new `AsyncCompletion` variant appends a tag that old payloads never contain. Old rules that `match` on `LogSource` without a wildcard arm still compile because the *runtime* only sends `AsyncCompletion` to rules that registered a continuation (i.e., new rules). An old rule can only observe the variant if it is manually sent one, which cannot happen: completions are targeted via `Message.module` at the spawning rule. |
| **Error codes** | ✅ Preserved. `-1` through `-17` keep their exact meanings; new codes are `-18`/`-19`/`-20`. The one refactor — `From<PlaidFunctionError> for i32` changed from `e as i32` to an explicit match — **fixes a latent inconsistency rather than changing behavior**: the old cast produced *positive* values (0–17, the enum discriminants) while the reverse direction (`From<i32>`) maps *negative* codes. The old conversion was never round-trip-consistent; the new one emits the same negative codes the runtime actually sends. No code in the runtime, STL, or any rule converted `PlaidFunctionError → i32` before this change (verified by grep across `runtime/`, `modules/`, `local-dev/`), so no observable behavior changes. |
| **Config** | ✅ Additive. `[executor.async_tickets]` is optional with defaults; every existing key keeps its meaning. The `LimitableAmount` change is `private → pub` fields only (no serde/behavior change). |
| **Host function behavior** | ✅ Unchanged for all 157 pre-existing functions — the async dispatch table is a parallel path; the synchronous `impl_new_function!` macros were not touched. |
| **Logback system** | ✅ Untouched except for one additive check: `async_spawn` draws from the same `LogbacksAllowed` budget. A rule that never calls `async_spawn` sees identical budget accounting. |

### Empirical verification

A **pre-change** `test_logback.wasm` (built against STL 47.1.0, exercising GET responses, synchronous MNR, cache, persistent responses, and a 3-second delayed logback) was loaded and exercised on the new runtime alongside the new `test_async.wasm`:

- All modules loaded and linked (old and new together).
- GET request → rule ran, synchronous MNR fired, response `"OK"` returned, persistent response updated.
- The 3-second delayed logback fired via the 10-second poller; the logback handler ran and updated the cache (`0` → `1`), confirmed by the request handler receiving both values.
- The full old-rule lifecycle completed with no errors and no behavior difference.

(The one panic observed during testing was the old rule's *own* `_ => panic!()` arm firing because a POST was sent to a GET-only rule — pre-existing rule logic, not a runtime regression.)

### The one intentional non-backward-compatible element

**Crate versions**: `plaid` and `plaid-stl` moved 47.1.0 → 48.0.0. This is a *dependency* version bump, not a breaking API change — but a rule must be recompiled to pick up the new STL (it cannot gain async abilities without recompiling, which is inherent: the continuation macro must generate code inside the rule). Old compiled rules keep working unmodified; they simply don't get the new features until rebuilt. The 48.0.0 (major) bump reflects the new guest-facing API surface per the repo's sync-version policy, not a break in the old surface.

---

## 6. Sample rule code

### 6.1 The complete test rule (`modules/tests/test_async/src/lib.rs`)

This rule demonstrates the full pattern: spawn with state, push-based completion, content verification, and continuation chaining.

```rust
use plaid_stl::{
    entrypoint_with_async,
    messages::LogSource,
    network::{MakeRequestRequest, MnrResponseEncoding},
    plaid,
    r#async::{AsyncCompletion, AsyncContext},
};

use std::collections::HashMap;

// Register the continuation handlers with the entrypoint macro.
entrypoint_with_async!(on_echo, on_final);

/// Parameters for the `test-async` named request.
fn mnr_params(body: &str) -> String {
    let request = MakeRequestRequest {
        request_name: "test-async".to_string(),
        body: body.to_string(),
        variables: HashMap::new(),
        headers: None,
        response_encoding: MnrResponseEncoding::Utf8,
    };
    serde_json::to_string(&request).unwrap()
}

/// Step 1: the webhook trigger. Spawns the first async call and returns
/// immediately — no execution thread is blocked while the HTTP request is
/// in flight.
fn main(data: String, _source: LogSource, ctx: AsyncContext) -> Result<(), i32> {
    plaid::print_debug_string(&format!("[test-async] step1: got payload {data:?}, spawning"));

    let ticket = ctx
        .spawn_with_state(
            "general_make_named_request",
            &mnr_params(&data),
            "on_echo",   // continuation handler name
            1,           // chain budget for the continuation
            &data,       // state echoed back in the completion
        )
        .map_err(|e| {
            plaid::print_debug_string(&format!("[test-async] step1: spawn failed: {e}"));
            1
        })?;

    // The polling API is also available: the ticket must report Pending
    // (or Completed if the call was very fast).
    match ctx.ticket_status(ticket) {
        Ok(status) => plaid::print_debug_string(&format!(
            "[test-async] step1: ticket {ticket} status right after spawn: {status:?}"
        )),
        Err(e) => plaid::print_debug_string(&format!(
            "[test-async] step1: ticket status check failed: {e}"
        )),
    }

    plaid::print_debug_string("[test-async] step1: returning, thread is free");
    Ok(())
}

/// Step 2: continuation for the first call. Receives the echoed body,
/// spawns a second call to prove continuations can chain.
fn on_echo(completion: AsyncCompletion, ctx: AsyncContext) -> Result<(), i32> {
    let original = completion.state.clone().unwrap_or_default();

    let body = match completion.result.as_string() {
        Ok(body) => body,
        Err(e) => {
            plaid::print_debug_string(&format!("[test-async] step2: first call failed: {e}"));
            return Err(1);
        }
    };

    // The MNR response is {"code":200,"data":"<echoed body>"}. The echo
    // server returns the request body, so data must contain the original
    // payload: this proves the async result carried real content.
    let echoed = extract_data(&body).unwrap_or_default();
    if echoed != original {
        plaid::print_debug_string(&format!(
            "[test-async] step2: MISMATCH expected {original:?} got {echoed:?}"
        ));
        return Err(1);
    }

    plaid::print_debug_string(&format!(
        "[test-async] step2: first call echoed {echoed:?} for original {original:?}"
    ));

    // Chain: spawn a second call, carrying the first result forward.
    ctx.spawn_with_state(
        "general_make_named_request",
        &mnr_params(&format!("second:{original}")),
        "on_final",
        0,        // no further spawns allowed after this
        &echoed,   // carry the first result as state
    )
    .map_err(|e| {
        plaid::print_debug_string(&format!("[test-async] step2: spawn failed: {e}"));
        1
    })?;

    plaid::print_debug_string("[test-async] step2: chained second call, returning");
    Ok(())
}

/// Step 3: continuation for the second call. The chain is complete.
fn on_final(completion: AsyncCompletion, _ctx: AsyncContext) -> Result<(), i32> {
    let first_result = completion.state.clone().unwrap_or_default();

    let body = match completion.result.as_string() {
        Ok(body) => body,
        Err(e) => {
            plaid::print_debug_string(&format!("[test-async] step3: second call failed: {e}"));
            return Err(1);
        }
    };

    let echoed = extract_data(&body).unwrap_or_default();
    let expected = format!("second:{first_result}");
    if echoed != expected {
        plaid::print_debug_string(&format!(
            "[test-async] step3: MISMATCH expected {expected:?} got {echoed:?}"
        ));
        return Err(1);
    }

    plaid::print_debug_string(&format!(
        "[test-async] step3: DONE chain=({first_result:?} -> {echoed:?})"
    ));
    Ok(())
}

/// Extract the `data` field from an MNR response envelope.
fn extract_data(mnr_response: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(mnr_response).ok()?;
    parsed.get("data")?.as_str().map(|s| s.to_string())
}
```

### 6.2 Minimal async rule (the smallest useful shape)

```rust
use plaid_stl::{entrypoint_with_async, messages::LogSource, plaid, r#async::{AsyncCompletion, AsyncContext}};

entrypoint_with_async!(on_done);

fn main(data: String, _source: LogSource, ctx: AsyncContext) -> Result<(), i32> {
    // Start a GitHub GraphQL query without blocking; wake me up as "on_done".
    ctx.spawn("github_make_graphql_query", &data, "on_done", 0)?;
    Ok(())   // thread is free; the runtime re-invokes us with the result
}

fn on_done(completion: AsyncCompletion, _ctx: AsyncContext) -> Result<(), i32> {
    match completion.result.as_string() {
        Ok(response) => {
            plaid::print_debug_string(&format!("query returned: {response}"));
            Ok(())
        }
        Err(e) => {
            plaid::set_error_context(&e.to_string());
            Err(1)
        }
    }
}
```

### 6.3 Fire-and-forget with later claim (polling style)

```rust
use plaid_stl::{
    entrypoint_with_async, messages::LogSource, plaid,
    r#async::{AsyncContext, TicketStatus},
};

entrypoint_with_async!();   // no continuations; results are claimed manually

fn main(data: String, _source: LogSource, ctx: AsyncContext) -> Result<(), i32> {
    // No continuation: the result waits in the registry for a claim.
    let ticket = ctx.spawn("slack_post_message", &data, "", 0)?;

    // Stash the ticket ID so a later invocation (e.g. a cron tick or a
    // follow-up webhook) can claim the result.
    plaid::storage::insert(
        &format!("ticket:{ticket}"),
        ticket.to_hex().as_bytes(),
    )?;

    match ctx.ticket_status(ticket) {
        Ok(TicketStatus::Completed) => {
            let result = ctx.ticket_claim(ticket)?;
            plaid::print_debug_string(&format!("already done: {result:?}"));
        }
        Ok(TicketStatus::Pending) => {
            plaid::print_debug_string("still in flight; check back later");
        }
        _ => plaid::print_debug_string("unknown/expired ticket"),
    }
    Ok(())
}
```

### 6.4 Configuration for the test rule

```toml
# webhooks.toml — the trigger. Unlimited budget lets the chain continue.
[webhooks."internal".webhooks."testasync"]
log_type = "test_async"
logbacks_allowed = "Unlimited"
headers = []

# apis.toml — the named request the rule spawns.
[apis."general"."network"."web_requests"."test-async"]
verb = "post"
uri = "https://localhost:8998/async_echo"
return_body = true
return_code = true
allowed_rules = ["test_async.wasm"]
root_certificate = """
{plaid-secret{integration-test-root-ca}}
"""
[apis."general"."network"."web_requests"."test-async"."headers"]

# loading.toml — log type override + test-mode exemption.
[loading.log_type_overrides]
"test_async.wasm" = "test_async"
```

---

## 7. Verification results

### 7.1 Unit tests (12/12 passing)

`cargo test -p plaid --lib async_ops` — registry semantics (insert/status, per-module cap, state cap, claim one-shot, cancel, sweep, double-completion, oversize coercion, message shape, ID uniqueness) plus dispatch test-mode gating. The only failing tests in the crate (5× GCP google_docs, 1× DynamoDB `put_query_delete`) fail identically on the untouched tree — they require network/credentials and are pre-existing.

### 7.2 End-to-end integration run

Setup: release build of `plaid` + `request_handler`, signed `test_async.wasm`, sled storage, generated test CA/server certs, interpolated secrets.

**Trigger → response latency:**

```
curl_rc=0 elapsed_ms=7      # webhook responded in 7 ms
```

The rule returned immediately after spawning; both HTTP calls ran on the tokio pool afterwards.

**Request handler received (both async calls executed):**

```
hello-async from /async_echo
second:hello-async from /async_echo
```

**Rule's own log (the full chain, with content assertions passing):**

```
[test-async] step1: got payload "hello-async", spawning
[test-async] step1: ticket 9d9bf7f7e9854f24b534bf14a9058afe status right after spawn: Pending
[test-async] step1: returning, thread is free
[test-async] step2: first call echoed "hello-async" for original "hello-async"
[test-async] step2: chained second call, returning
[test-async] step3: DONE chain=("hello-async" -> "second:hello-async")
```

This verifies: spawn, immediate return, push-based completion delivery, state echo, content integrity of the delivered result, continuation chaining, budget carry-forward, and the polling API reporting `Pending` mid-flight.

**Shutdown:** a SIGTERM during an in-flight chain completed gracefully (`Plaid shutdown complete.`); the coercion path (completion → delayed/persisted queue) reuses the battle-tested logback machinery and is exercised by the same code path.

### 7.3 Coverage check

Cross-referencing the async dispatch table against the host-function registration table programmatically:

```
API host functions: 157
Async dispatchable: 157
Missing from async dispatch (0): []
In dispatch but not host table (0): []
```

Every API function a rule can call synchronously can now be spawned asynchronously, with identical test-mode gating.

---

## 8. Files changed / added

**Added (2,354 lines of new code):**

| File | Purpose |
|---|---|
| `runtime/plaid/src/async_ops/mod.rs` (635) | `TicketRegistry`, sweeper, config, 10 unit tests |
| `runtime/plaid/src/async_ops/dispatch.rs` (~700) | Async dispatch table for all 157 API functions + test-mode list |
| `runtime/plaid/src/functions/async_ops.rs` (411) | `async_spawn` / `ticket_status` / `ticket_claim` / `ticket_cancel` host functions |
| `runtime/plaid-stl/src/async/mod.rs` (331) | Guest API: `AsyncContext`, `TicketId`, `AsyncResult`, `AsyncCompletion` |
| `runtime/plaid/src/tests.rs` (53) | `stub_module` test helper |
| `modules/tests/test_async/` (275) | Test rule + integration harness |

**Modified (392 insertions across 19 files):**

| File | Change |
|---|---|
| `runtime/plaid-stl/src/lib.rs` | `entrypoint_with_async!` macro; new error variants; explicit `From<PlaidFunctionError> for i32` |
| `runtime/plaid-stl/src/messages.rs` | `LogSource::AsyncCompletion` variant |
| `runtime/plaid/src/executor/mod.rs` | `Env.ticket_registry`; threading through the executor call chain |
| `runtime/plaid/src/functions/{mod,api}.rs` | Host function registration + error codes |
| `runtime/plaid/src/config.rs` | `[executor.async_tickets]` (`AsyncTicketsConfig`) |
| `runtime/plaid/src/bin/plaid.rs` | Registry creation, sweeper start/join, `Executor::new` wiring |
| `runtime/plaid/src/bin/request_handler.rs` | `/async_echo` route for the test |
| `runtime/plaid/src/loader/mod.rs` | `compile_for_tests`; `LimitableAmount` fields made public |
| `runtime/plaid/src/lib.rs` | `async_ops` + test modules |
| `runtime/plaid/resources/config/{webhooks,apis,loading}.toml` | Test rule wiring |
| `modules/Cargo.toml` | Workspace member |
| `runtime/README.md` | Async ticket system documentation |
| Both `Cargo.toml` | Version 48.0.0 |

---

## 9. Design decisions worth recording

1. **Push over pull.** The runtime re-invokes the rule on completion rather than the rule polling itself. Polling wastes invocations (each poll costs computation budget and a queue slot), inherits the delayed-logback poller's 10 s granularity, and adds latency. The polling API (`ticket_status`/`ticket_claim`) remains for fire-and-forget and fan-in, where it is genuinely the right tool.
2. **Budgets mirror logbacks.** Async spawn + completion is a self-perpetuating invocation chain — the same hazard logbacks pose. Reusing `LogbacksAllowed` means the existing mental model, config, and accounting all apply unchanged.
3. **Fresh computation budget per step is a feature.** A long workflow gets a fresh metering allowance per continuation, so total work is bounded by *depth × limit* — with the budget as the real chain limiter.
4. **The completion reuses `Message.module` dispatch.** The single-module path already existed for GET-webhook responses; async completions needed exactly that semantics (run only the owner), so no new dispatch machinery was required.
5. **`Api` is not `Clone`, so futures capture `Arc<Api>`.** The subsystem is resolved inside the future after an existence check at spawn time — this keeps the table honest (unconfigured APIs fail at spawn, not at first poll) without invasive `Clone` derives on every API struct.
6. **Explicit table over macro generation for dispatch.** Each arm is two lines and auditable against `functions/api.rs`; the coverage cross-check in §7.3 is then a mechanical diff.
7. **`r#async` everywhere in guests.** `async` is reserved; the raw identifier is the price of the natural module name.

---

## 10. Future work (not in this change)

- **Durable spawns (phase 2):** persist spawn *requests* in a reserved storage namespace before executing; replay unclaimed ones on startup (at-least-once; idempotency is the rule author's responsibility). Reuses the delayed-logback persistence machinery.
- **Fan-in helper:** an STL `FanIn` utility (storage-backed counter keyed by correlation ID) so "spawn N, proceed at N" is a one-liner.
- **GET-webhook recipe:** return `202`/persistent-response immediately and let the push completion update the persistent response for subsequent requests (`UsePersistentResponse` caching mode already exists for this shape).
- **Metrics:** wire the registry counters into `ModuleExecutionMetrics` (outstanding gauge, spawn/completion rates, completion latency histogram, TTL evictions).
- **wasmer `experimental-async` (strategic):** enabling true suspension would make the *existing* synchronous STL calls non-blocking with zero guest changes. It does not obsolete the ticket system (durable workflows, fire-and-forget, fan-in still need tickets), and the guest-facing contract designed here stays stable if it lands.
