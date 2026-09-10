//! # Async Ticket System Test
//!
//! Demonstrates and verifies the async ticket system: a rule can start a
//! long-running API call and return immediately, without blocking an
//! execution thread. When the call completes, the runtime re-invokes the
//! rule and the result is processed by a continuation handler.
//!
//! ## What this rule does
//!
//! 1. On a webhook trigger, `main` spawns a `general_make_named_request`
//!    call (the `test-async` MNR, which echoes its body back) with the
//!    continuation `on_echo`. It attaches the original payload as state so
//!    the continuation can correlate the result with the request. `main`
//!    then returns — the execution thread is free.
//! 2. When the HTTP call completes, the runtime re-invokes the rule with a
//!    completion message. The `entrypoint_with_async!` macro routes it to
//!    `on_echo`, which spawns a *second* request with the continuation
//!    `on_final` (proving continuations can chain), carrying the first
//!    result as state.
//! 3. `on_final` receives the second result and reports the full chain via
//!    `print_debug_string`, which the test harness greps for.
//!
//! This exercises: spawn, push-based completion delivery, state echo,
//! continuation chaining, and the budget system (each spawn costs budget).
//!
//! ## Config required
//! ```toml
//! # webhooks.toml
//! [webhooks."internal".webhooks."testasync"]
//! log_type = "test_async"
//! logbacks_allowed = "Unlimited"
//! headers = []
//!
//! # apis.toml
//! [apis."general"."network"."web_requests"."test-async"]
//! verb = "post"
//! uri = "https://localhost:8998/async_echo"
//! return_body = true
//! return_code = true
//! allowed_rules = ["test_async.wasm"]
//! root_certificate = """
//! {plaid-secret{integration-test-root-ca}}
//! """
//! [apis."general"."network"."web_requests"."test-async"."headers"]
//! ```
//!
//! ## Try it
//! ```sh
//! curl -d 'hello-async' http://localhost:4554/webhook/testasync
//! # Then check the logs for the [test-async] lines, or watch the
//! # request handler's /async_echo output.
//! ```

use plaid_stl::{
    entrypoint_with_async,
    messages::LogSource,
    network::{MakeRequestRequest, MnrResponseEncoding},
    plaid,
    r#async::{AsyncCompletion, AsyncContext},
};

use std::collections::HashMap;

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
            "on_echo",
            1,
            &data,
        )
        .map_err(|e| {
            plaid::print_debug_string(&format!("[test-async] step1: spawn failed: {e}"));
            1
        })?;

    // Demonstrate the polling API on the ticket we just spawned: it must
    // report Pending (or already Completed if the call was very fast).
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
            plaid::print_debug_string(&format!(
                "[test-async] step2: first call failed: {e}"
            ));
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
        0,
        &echoed,
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
            plaid::print_debug_string(&format!(
                "[test-async] step3: second call failed: {e}"
            ));
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

/// Extract the `data` field from an MNR response envelope
/// (`{"code":200,"data":"..."}`).
fn extract_data(mnr_response: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(mnr_response).ok()?;
    parsed.get("data")?.as_str().map(|s| s.to_string())
}
