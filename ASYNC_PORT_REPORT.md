# Plaid Async Port — Implementation Report

**Branch:** `async-port-wasmer7` (off `main` @ `3c51fc28`) · **Date:** 2026-10-08
**Scope:** Port the Wasmer 7 `experimental-async` guest-suspension model (proven in the `wasmer7` POC) into Plaid, so rules that call external APIs **suspend** instead of blocking executor threads. Implemented per `PLAID_ASYNC_PORT_PLAN.md` (§4.1–§4.9, Phases 0–2).

---

## 1. What changed

### 1.1 `runtime/plaid/Cargo.toml` + `runtime/Cargo.lock` (§4.1)

- `wasmer` pinned to **`=7.5.0`** (was floating `"7"` → resolved 7.0.1) and `wasmer-middlewares` pinned to **`=7.5.0`** in lockstep. The experimental-async API churns across 7.x; the POC verified signatures against 7.5.0 exactly.
- `experimental-async` wired through the existing passthrough features:
  - `cranelift = ["wasmer/cranelift", "wasmer/experimental-async"]`
  - `llvm = ["wasmer/llvm", "wasmer/experimental-async"]`
- `Cargo.lock` committed with both crates at 7.5.0.

### 1.2 `functions/memory.rs` — async helper variants (§4.2)

Added async counterparts next to the sync ones (sync ones stay for the sync-registered host fns):

- `safely_get_string_async(env: &AsyncFunctionEnvMut<Env>, ptr, len) -> Result<String, FunctionErrors>`
- `safely_write_data_back_async(env, data, ptr, len) -> Result<i32, FunctionErrors>`
- `safely_get_memory_async(env, ptr, len, max) -> Result<Vec<u8>, FunctionErrors>`

All follow the POC guard discipline: acquire `env.read().await` → copy bytes in/out → drop the guard. **Never hold a guard across an `.await` of host services** — the guard holds the store lock, which would block other coroutines against the same store.

Note on `WasmPtr<u8>`: the plan flagged the `FromToNativeWasmType + 'static` bound as a Phase-0 risk. Verified in the wasmer 7.5.0 source (`utils/mem/ptr.rs`: `unsafe impl<T: ValueType, M: MemorySize> FromToNativeWasmType for WasmPtr<T, M>`) — **`WasmPtr<u8>` satisfies the async bounds**, so the async fns keep the exact same `WasmPtr<u8>` signatures as their sync counterparts. No `u32` fallback needed.

### 1.3 `functions/api.rs` — the four macros converted (§4.3, §4.5)

All four macros (`impl_new_function`, `impl_new_function_with_error_buffer`, `impl_new_sub_module_function`, `impl_new_sub_module_function_with_error_buffer`) now generate **async host functions** registered via `Function::new_typed_with_env_async`. Shape of each generated function:

1. **Guard block:** log the call, check test mode, snapshot the handles needed (`Arc<Api>` clone, `Arc<PlaidModule>` clone, `Logger` clone) — guard dropped at block end.
2. **Params read** via `safely_get_string_async` (own short-lived guard).
3. **NO guard held:** `api.$function_name(&params, module).await` — this is where Wasmer parks the guest's stack and frees the executor thread.
4. **Result mapping byte-identical** to the sync version (`ApiError::TestMode` → `FunctionErrors::TestMode`, etc.), error codes unchanged.

The two-function `_impl`/wrapper split is kept (wrapper returns the final `i32` error code, exactly like the old sync wrappers), but the nested `env_api.runtime.block_on(...)` and the "Clone the APIs Arc to use in Tokio closure" dance are gone — replaced by a genuine `.await`.

`define_api_functions!` gained a third category:

- `with_env` — unchanged sync fns: `fetch_data`, `fetch_source`, `get_headers`, `get_query_params`, `get_response`, `set_response`, `set_response_status`, `set_error_context`, `get_accessory_data`, `get_secrets`, `fetch_random_bytes`, `log_back`, `print_debug_string`, …
- `with_env_async` — **all API fns** (github_*, slack_*, okta_*, aws_*, gcp_*, jira_*, npm_*, pagerduty_*, rustica_*, splunk_*, yubikey_*, web_*, blockchain_*, cryptography_*, bloom_filter_*, general_*) **plus storage_\* and cache_\*** (they await the backing systems)
- `without_env` — `get_time` (stays sync, no env)

`is_known_api_function` generation unchanged (same names — binary compat preserved).

### 1.4 `functions/cache.rs` (§4.4)

`cache_insert` / `cache_get` converted to async host fns. The 5s `tokio::time::timeout` is **kept but awaited** — it now actually cancels the in-flight work instead of merely unblocking the thread (subtle improvement noted in the plan).

### 1.5 `functions/storage/*` (§4.4)

All ten storage host fns converted: `insert`, `insert_shared`, `insert_batch`, `insert_batch_shared`, `get`, `get_shared`, `delete`, `delete_shared`, `list_keys`, `list_keys_shared`.

- The `storage_current` byte accounting and limit checks stay inside the guard sections, exactly as before.
- The `std::sync::RwLock` counter guards in `insert_common`/`delete_common` are unchanged (they're plain sync locks over `Arc<RwLock<u64>>`, independent of the store lock).
- The old sync `safely_get_guest_string!`/`safely_get_guest_memory!` macros were removed (all callers converted); async helpers re-exported from `storage/mod.rs`.

### 1.6 `executor/mod.rs` — the mandatory call-site change (§4.6)

- **`PreparedExecution` bundle** replaces the old 4-tuple: `store_async: StoreAsync`, `instance`, `entrypoint: TypedFunction<(), i32>`, `env: FunctionEnv<Env>`. `Store::into_async()` consumes the `Store` at the end of `prepare_for_execution`; everything after goes through `StoreAsync` locks.
- **`process_message_with_module` is now `async fn`** and invokes the entrypoint via `prepared.entrypoint.call_async(&prepared.store_async).await` — the async boundary. A synchronous `entrypoint.call()` of a rule that suspends would trap ("cannot yield when not in async context"), which is why this change is atomic with §1.3.
- **Post-call bookkeeping behind locks:** response/`execution_error_context`/`invalid_response_status` reads via `store_async.read_lock().await` + `env.as_ref(&lock)`; metering (`get_remaining_points`) via `store_async.write_lock().await` (`StoreAsyncWriteLock: AsStoreMut`, so the wasmer-middlewares API composes unchanged). Metrics values identical to before.
- **`execution_loop`:** each worker thread creates a **current-thread tokio runtime once at thread start** and drives each message via `rt.block_on(process_message_with_module(...))`. Blocking `recv()` → prepare → `rt.block_on` → loop. A thread only ever exits between messages, never mid-message — drain semantics preserved.
- **SAFETY INVARIANT (documented in code):** never `rt.spawn` on the per-thread runtime. A current-thread runtime dropped with live tasks abandons them; since we only `block_on`, there are zero pending tasks at drop. This is what keeps shutdown/drain correct (§4.9).

### 1.7 `apis/mod.rs` — `Api::runtime` removed (§4.8)

The `runtime: Runtime` field existed solely to serve the `block_on`s. With no callers left, the field, the `Runtime::new()` in `Api::new`, and the `ApiError::CouldNotInstatiateRuntime` variant are deleted. This also removes the nested-runtime worker threads (~#cores) from the process.

### 1.8 `loader/mod.rs` — 7.0.1 → 7.5.0 API drift fix

`BaseTunables::for_target(&Target::default())` no longer exists in 7.5.0 (tunables became target-independent). Replaced with `BaseTunables::new()`; the now-unused `Target` import removed. This is the only wasmer-version-drift fix the port required.

---

## 2. What stayed untouched (per §5)

- **`plaid-stl` (guest side): zero changes.** The wasm-level import signatures of async-registered host fns are identical to their sync counterparts; suspension is invisible to the guest. Rules need no source changes and no recompile.
- Sync host functions (message/response/runtime_data/internal incl. `get_time`, `fetch_random_bytes`, `log_back`) stay sync — they work fine under `call_async`, they just run inline.
- `link_functions_to_module` name-matching, `is_known_api_function` load-time validation, wasm-bindgen placeholder namespaces — unchanged.
- Webhook/data-generator/interval/SQS/websocket ingress — they only interact via `MessageSender`; unchanged.
- Thread-pool routing (`thread_pools.rs`), `log_queue_size`, crossbeam channel architecture, shutdown/drain logic in `bin/plaid.rs` — unchanged (verified: shutdown works by channel disconnect, which the port doesn't touch).

---

## 3. Verification status

| Check | Result |
|---|---|
| `cargo check` — `sled,cranelift,aws,gcp` (default backend set) | ✅ clean |
| `cargo check` — `sled,cranelift` (no aws/gcp) | ✅ clean |
| `cargo build --release` — full `plaid` binary, `sled,cranelift,aws,gcp` | ✅ 12m28s, links cleanly |
| `cargo test` unit tests | ✅ 8 passed; 6 failed — **all 6 fail identically on `main`** (verified via `git stash` A/B: AWS DynamoDB + GCP Google Docs tests requiring live credentials/network; unrelated to the port) |
| `llvm` feature | ⚠️ blocked by environment: `llvm-sys` needs a system LLVM 22 (`LLVM_SYS_221_PREFIX`) not present in this container. Pre-existing limitation, not caused by the port. CI with an LLVM image should verify (plan Phase 4). |
| `grep -rn "block_on" runtime/plaid/src` | ✅ exactly the per-thread `rt.block_on` in the executor (2 call sites + 1 comment) — every host-fn `block_on` is gone (§3.1 acceptance criterion) |

**Not yet run** (deferred, per owner's "if it compiles, it's good enough for now"): the full `testing/integration.sh` suite with pre-built test modules (the binary-compat proof), the suspension-benefit timing test, and the §8.3 benchmark sweep. These are the plan's Phase 2/3 acceptance gates and should be run before merging to production.

---

## 4. Deviations from the plan

1. **`WasmPtr<u8>` kept (plan §4.2 hedged on this):** verified `WasmPtr<T, M>: FromToNativeWasmType` in wasmer 7.5.0 source, so async host fns use the same `WasmPtr<u8>` params as the sync ones — no `u32` fallback needed. Guest-visible signatures are byte-identical.
2. **Two-function `_impl`/wrapper split kept (plan §4.3 suggested collapsing it):** the wrapper still exists but is now async and returns the final `i32` directly. Collapsing fully would have required reworking error logging (which needs the module name from a guard); keeping the split preserved the exact error-code mapping and log lines with less churn.
3. **`max_in_flight_per_thread` knob (plan §4.7) NOT implemented:** the plan itself defaults it to 1 (= today's semantics exactly) and flags ordering-semantics concerns. This port delivers the suspension model + parity; the concurrency knob is a clean follow-up on top of it (the per-thread runtime + `call_async` plumbing it needs is now in place).
4. **Suspension metrics (plan §8.2) NOT implemented:** same reasoning — behavior-preserving port first; metrics are additive and can land independently.

With deviations 3+4, this port corresponds to the plan's **Phases 0–2 complete** (spike findings folded in, host functions converted, executor converted, `Api::runtime` removed), with Phase 3 (knob + metrics) and Phase 4 (hardening/benchmarks/docs) remaining as follow-ups.

---

## 5. Rollout / rollback notes

- **Rules are binary-compatible:** old `.wasm` files link and run unchanged (import signatures identical); new `.wasm` files would also run on the old runtime. Runtime and rules can be rolled out/rolled back independently.
- **Default semantics are identical to pre-port** (one message at a time per thread, strict ordering within a pool); the difference is the thread *parks* while a guest is suspended instead of spinning in a nested `block_on`, and the nested `Api::runtime` worker threads are gone.
- **Known observable deltas:** `plaid_module_execution_duration_seconds` now includes suspension time (wall clock) — unchanged in practice for `max_in_flight=1` semantics since the wall time is the same; cache timeouts now cancel in-flight work at 5s.
- **Toolchain contingency (plan §6):** if Plaid bumps its module-build toolchain and module builds start failing with `undefined symbol: github_get_repo`-style errors, the fix is adding `#[link(wasm_import_module = "env")]` to the extern blocks in `plaid-stl` — not needed on the current toolchain, not changed in this port.
