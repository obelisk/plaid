# Runtime

This is where the Plaid runtime code lives along with the STL used by modules. The reason the STL lives in here is because there are shared structures between the `plaid` and `plaid-stl` codebases and having the `plaid-stl` here allows it to be pulled in by modules, but the reverse was not true last time I tried. I also feel these have more in common that the modules do with the STL.

## Async operations (the ticket system)

Rules are compiled to `wasm32-unknown-unknown` and cannot yield during
execution: the host call is synchronous and the guest has no suspension
primitives. The ticket system reifies "async" on top of that limitation:

1. A rule calls `AsyncContext::spawn` (STL: `plaid_stl::r#async`). The
   runtime registers a ticket, starts the API call on the shared tokio
   runtime, and returns the ticket ID immediately. The rule's invocation
   ends without blocking an execution thread.
2. When the call completes, the runtime re-invokes the rule with a
   completion message (log type `__plaid_async`, source
   `LogSource::AsyncCompletion`). The `entrypoint_with_async!` macro routes
   it to the continuation handler registered at spawn time.

Key properties:

- **Push-based**: the runtime wakes the rule; no polling loop required.
  `ticket_status` / `ticket_claim` exist for fire-and-forget and fan-in
  patterns.
- **Budgeted**: spawns draw from the same budget as logbacks
  (`LogbacksAllowed`), bounding continuation-chain depth.
- **Bounded**: per-rule outstanding-ticket cap and a TTL on unclaimed
  results (see `[executor.async_tickets]` in the config).
- **Shutdown-safe**: completions that race shutdown are coerced to the
  delayed/persisted logback path, so pending continuations survive restart
  when storage is persistent.

See `modules/tests/test_async` for a complete example, including
continuation chaining and state echo.
