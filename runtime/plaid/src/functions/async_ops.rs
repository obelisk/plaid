//! Host functions for the async ticket system.
//!
//! These are the runtime side of `plaid_stl::async`:
//!
//! * `async_spawn` — start an API call on the tokio runtime, register a
//!   ticket, and return its ID to the guest. The rule's invocation ends
//!   without blocking the execution thread.
//! * `ticket_status` — poll a ticket.
//! * `ticket_claim` — one-shot retrieval of a result.
//! * `ticket_cancel` — best-effort cancellation.
//!
//! Completion delivery is push-based: when the spawned task finishes, the
//! completion message is injected into the executor queue via the
//! `immediate_sender` (or the delayed/persisted path during shutdown drain,
//! mirroring `dispatch_logback`).

use std::sync::Arc;

use wasmer::{AsStoreRef, FunctionEnvMut, WasmPtr};

use crate::async_ops::{
    prepare_async_api_call, spawn_sweeper, SpawnOutcome, TicketError, TicketId, TicketRegistry,
    TicketResult,
};
use crate::executor::Env;

use super::{get_memory, safely_get_memory, safely_get_string, safely_write_data_back};

/// Map registry errors onto the STL's error codes.
impl From<TicketError> for super::FunctionErrors {
    fn from(e: TicketError) -> Self {
        match e {
            TicketError::UnknownTicket => super::FunctionErrors::UnknownTicket,
            TicketError::TooManyPending => super::FunctionErrors::TooManyPendingTickets,
            TicketError::StateTooLarge => super::FunctionErrors::AsyncStateTooLarge,
            TicketError::ResultTooLarge => super::FunctionErrors::InternalApiError,
        }
    }
}

/// Read a 16-byte ticket ID from guest memory.
fn read_ticket(env: &FunctionEnvMut<Env>, ticket_ptr: WasmPtr<u8>) -> Result<TicketId, i32> {
    let store = env.as_store_ref();
    let memory_view = get_memory(env, &store).map_err(|e| e as i32)?;
    let mut bytes = [0u8; 16];
    memory_view
        .read(ticket_ptr.offset().into(), &mut bytes)
        .map_err(|_| super::FunctionErrors::InvalidPointer as i32)?;
    Ok(TicketId(bytes))
}

/// Spawn an async API call. See `plaid_stl::async::AsyncContext::spawn`.
///
/// The guest passes the host function name, its JSON params, the continuation
/// handler name (empty for fire-and-forget), the chain budget for the
/// continuation, an optional state payload, and receives the 16-byte ticket
/// ID written into `ticket_out`.
pub fn async_spawn(
    mut env: FunctionEnvMut<Env>,
    api_fn_buf: WasmPtr<u8>,
    api_fn_len: u32,
    params_buf: WasmPtr<u8>,
    params_len: u32,
    continuation_buf: WasmPtr<u8>,
    continuation_len: u32,
    budget: u32,
    state_buf: WasmPtr<u8>,
    state_len: u32,
    ticket_out: WasmPtr<u8>,
) -> i32 {
    let name = env.data().module.name.clone();

    // Phase 1: read everything the guest passed. The memory view borrows
    // the store, so this is scoped to drop it before we mutate the env.
    let (api_fn, params, continuation, state) = {
        let store = env.as_store_ref();
        let memory_view = match get_memory(&env, &store) {
            Ok(memory_view) => memory_view,
            Err(e) => {
                error!("{name}: Memory error in async_spawn: {e:?}");
                return e as i32;
            }
        };

        let max_buffer_size = super::calculate_max_buffer_size(env.data().module.page_limit);

        let api_fn = match safely_get_string(&memory_view, api_fn_buf, api_fn_len) {
            Ok(s) => s,
            Err(e) => {
                error!("{name}: Error reading API function name in async_spawn: {e:?}");
                return e as i32;
            }
        };

        let params = match safely_get_memory(&memory_view, params_buf, params_len, max_buffer_size)
        {
            Ok(d) => d,
            Err(e) => {
                error!("{name}: Error reading params in async_spawn: {e:?}");
                return e as i32;
            }
        };
        let params = match String::from_utf8(params) {
            Ok(s) => s,
            Err(_) => return super::FunctionErrors::ParametersNotUtf8 as i32,
        };

        let continuation =
            match safely_get_string(&memory_view, continuation_buf, continuation_len) {
                Ok(s) => s,
                Err(e) => {
                    error!("{name}: Error reading continuation in async_spawn: {e:?}");
                    return e as i32;
                }
            };

        let state = if state_len > 0 {
            match safely_get_memory(&memory_view, state_buf, state_len, max_buffer_size) {
                Ok(d) => match String::from_utf8(d) {
                    Ok(s) => Some(s),
                    Err(_) => return super::FunctionErrors::ParametersNotUtf8 as i32,
                },
                Err(e) => {
                    error!("{name}: Error reading state in async_spawn: {e:?}");
                    return e as i32;
                }
            }
        } else {
            None
        };

        (api_fn, params, continuation, state)
    };

    // Validate the state size against the registry limit before doing any
    // work so the rule gets a synchronous, actionable error.
    if let Some(state) = &state {
        if state.len() > crate::async_ops::MAX_STATE_BYTES {
            error!(
                "{name}: async spawn state payload of {} bytes exceeds the {} byte limit",
                state.len(),
                crate::async_ops::MAX_STATE_BYTES
            );
            return super::FunctionErrors::AsyncStateTooLarge as i32;
        }
    }

    let env_data = env.data();
    let Some(registry) = env_data.ticket_registry.as_ref() else {
        error!("{name}: async_spawn called but the ticket system is not configured");
        return super::FunctionErrors::OperationNotAllowed as i32;
    };
    // Clone everything the spawn needs so no borrow of the env outlives the
    // budget mutation below. The tokio Runtime is not Clone, so it is
    // borrowed in its own scope when the task is spawned.
    let registry = registry.clone();
    let api = env_data.api.clone();
    let module = env_data.module.clone();
    let immediate_sender = env_data.immediate_sender.clone();
    let delayed_log_sender = env_data.delayed_log_sender.clone();
    let cancellation_token = env_data.cancellation_token.clone();

    // Deduct the chain budget, mirroring the logback budget system: the
    // continuation gets the budget the rule asked for, minus what this
    // spawn's continuation chain will consume. Unlimited stays unlimited.
    let assigned_budget = match &mut env.data_mut().message.logbacks_allowed {
        plaid_stl::messages::LogbacksAllowed::Unlimited => {
            plaid_stl::messages::LogbacksAllowed::Unlimited
        }
        plaid_stl::messages::LogbacksAllowed::Limited(remaining) => {
            if budget > *remaining {
                error!(
                    "{name}: async spawn budget exceeded. Requested {budget}, but only {remaining} was available."
                );
                return super::FunctionErrors::LogbackBudgetExhausted as i32;
            }
            *remaining -= budget;
            plaid_stl::messages::LogbacksAllowed::Limited(budget)
        }
    };

    // Validate the requested API function before registering the ticket so
    // unknown names fail synchronously.
    let Some(outcome) = prepare_async_api_call(&api, &api_fn, &params, module.clone()) else {
        error!("{name}: async_spawn requested unknown API function: {api_fn}");
        return super::FunctionErrors::OperationNotAllowed as i32;
    };

    // Register the ticket.
    let ticket = match registry.insert(&name, &continuation, state, assigned_budget) {
        Ok(t) => t,
        Err(e) => {
            error!("{name}: async_spawn failed to register ticket: {e}");
            return super::FunctionErrors::from(e) as i32;
        }
    };

    match outcome {
        SpawnOutcome::Future(fut) => {
            // Drive the call on the shared tokio runtime. When it finishes,
            // complete the ticket and inject the completion message into the
            // executor queue. The runtime handle is borrowed in its own
            // scope; the task itself is detached (the sweeper reaps
            // abandoned tickets).
            let spawned = env.data().api.runtime.spawn(async move {
                let result = fut.await;
                deliver_completion(
                    &registry,
                    ticket,
                    result,
                    module,
                    immediate_sender,
                    delayed_log_sender,
                    cancellation_token,
                );
            });
            // Detach: the runtime keeps driving it independent of this
            // execution. The sweeper reaps abandoned tickets.
            std::mem::forget(spawned);
        }
        SpawnOutcome::ImmediateError(error) => {
            // The call can never succeed (test mode, unconfigured API).
            // Complete the ticket right away so the rule hears back.
            deliver_completion(
                &registry,
                ticket,
                TicketResult::err(error),
                module,
                immediate_sender,
                delayed_log_sender,
                cancellation_token,
            );
        }
    }

    // Phase 2: write the ticket ID back to the guest. Done after spawning
    // so the rule has the ID even if this write somehow fails; a failure
    // here only means the rule cannot poll/cancel this ticket.
    {
        let store = env.as_store_ref();
        let memory_view = match get_memory(&env, &store) {
            Ok(memory_view) => memory_view,
            Err(e) => {
                error!("{name}: Memory error writing ticket ID in async_spawn: {e:?}");
                return e as i32;
            }
        };
        if let Err(e) = safely_write_data_back(&memory_view, ticket.as_bytes(), ticket_out, 16) {
            error!("{name}: Failed to write ticket ID back to guest: {e:?}");
            return e as i32;
        }
    }

    0
}

/// Complete a ticket and route the completion message. Mirrors
/// `dispatch_logback`: immediate delivery when possible, delayed/persisted
/// queue during shutdown drain.
#[allow(clippy::too_many_arguments)]
fn deliver_completion(
    registry: &Arc<TicketRegistry>,
    ticket: TicketId,
    result: TicketResult,
    module: Arc<crate::loader::PlaidModule>,
    immediate_sender: Option<crossbeam_channel::Sender<crate::executor::Message>>,
    delayed_log_sender: crossbeam_channel::Sender<crate::data::DelayedMessage>,
    cancellation_token: tokio_util::sync::CancellationToken,
) {
    let Some(message) = registry.complete(ticket, result, module) else {
        // Fire-and-forget or already handled: nothing to deliver.
        return;
    };

    let cancelled = cancellation_token.is_cancelled();

    // Happy path: not shutting down, immediate sender available.
    if let (false, Some(sender)) = (cancelled, &immediate_sender) {
        if let Err(e) = sender.try_send(message) {
            let err = e.to_string();
            let source = e.into_inner().source;
            error!("Failed to deliver async completion from {source}. Error: {err}");
        }
        return;
    }

    // Shutdown drain: coerce to the delayed/persisted path (delay of 1
    // second, matching the logback coercion) so pending continuations
    // survive restart when storage is persistent.
    if cancelled {
        warn!(
            "Shutdown in progress: coercing async completion for {ticket} to the delayed queue"
        );
    }

    let delayed = crate::data::DelayedMessage::new(1, message);
    if let Err(e) = delayed_log_sender.try_send(delayed) {
        let err = e.to_string();
        let source = e.into_inner().message.source;
        error!("Delayed async completion dispatch from {source} failed; message dropped. Error: {err}");
    }
}

/// Poll the status of a ticket. Returns 0=Pending, 1=Completed, 2=Failed,
/// 3=Unknown, or a negative error code.
pub fn ticket_status(env: FunctionEnvMut<Env>, ticket_ptr: WasmPtr<u8>) -> i32 {
    let name = env.data().module.name.clone();

    let Some(registry) = env.data().ticket_registry.as_ref() else {
        error!("{name}: ticket_status called but the ticket system is not configured");
        return super::FunctionErrors::OperationNotAllowed as i32;
    };

    let ticket = match read_ticket(&env, ticket_ptr) {
        Ok(t) => t,
        Err(e) => return e,
    };

    use crate::async_ops::Status;
    match registry.status(ticket) {
        Status::Pending => 0,
        Status::Completed => 1,
        Status::Failed => 2,
        Status::Unknown => 3,
    }
}

/// Claim the result of a ticket, removing it from the registry. The result
/// is a JSON-encoded `TicketResult` written into `result_buf`. Like the
/// other buffer-returning host functions, a zero-length buffer returns the
/// required size.
pub fn ticket_claim(
    env: FunctionEnvMut<Env>,
    ticket_ptr: WasmPtr<u8>,
    result_buf: WasmPtr<u8>,
    result_buf_len: u32,
) -> i32 {
    let name = env.data().module.name.clone();

    let Some(registry) = env.data().ticket_registry.as_ref() else {
        error!("{name}: ticket_claim called but the ticket system is not configured");
        return super::FunctionErrors::OperationNotAllowed as i32;
    };

    let ticket = match read_ticket(&env, ticket_ptr) {
        Ok(t) => t,
        Err(e) => return e,
    };

    let result = match registry.claim(ticket) {
        Ok(r) => r,
        Err(e) => {
            // UnknownTicket is an expected outcome for polling-style rules;
            // surface it as a code rather than logging an error.
            return super::FunctionErrors::from(e) as i32;
        }
    };

    let serialized = match serde_json::to_vec(&result) {
        Ok(s) => s,
        Err(e) => {
            error!("{name}: Failed to serialize ticket result: {e}");
            return super::FunctionErrors::ErrorCouldNotSerialize as i32;
        }
    };

    let store = env.as_store_ref();
    let memory_view = match get_memory(&env, &store) {
        Ok(memory_view) => memory_view,
        Err(e) => {
            error!("{name}: Memory error in ticket_claim: {e:?}");
            return e as i32;
        }
    };

    match safely_write_data_back(&memory_view, &serialized, result_buf, result_buf_len) {
        Ok(x) => x,
        Err(e) => {
            error!("{name}: Error in ticket_claim: {e:?}");
            e as i32
        }
    }
}

/// Best-effort cancellation of a ticket.
pub fn ticket_cancel(env: FunctionEnvMut<Env>, ticket_ptr: WasmPtr<u8>) -> i32 {
    let name = env.data().module.name.clone();

    let Some(registry) = env.data().ticket_registry.as_ref() else {
        error!("{name}: ticket_cancel called but the ticket system is not configured");
        return super::FunctionErrors::OperationNotAllowed as i32;
    };

    let ticket = match read_ticket(&env, ticket_ptr) {
        Ok(t) => t,
        Err(e) => return e,
    };

    match registry.cancel(ticket) {
        Ok(()) => 0,
        Err(e) => super::FunctionErrors::from(e) as i32,
    }
}

/// Spawn the registry sweeper. Called once from `main` during startup.
pub fn start_sweeper(
    registry: Arc<TicketRegistry>,
    cancellation_token: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    spawn_sweeper(registry, cancellation_token)
}
