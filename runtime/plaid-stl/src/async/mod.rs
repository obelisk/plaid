//! Async operations for Plaid rules.
//!
//! Rules are compiled to `wasm32-unknown-unknown` and cannot yield during
//! execution: the only way to "await" a long-running operation is to return
//! from the entrypoint and be re-invoked when the operation completes. This
//! module implements that pattern on top of the runtime's ticket system:
//!
//! 1. [`AsyncContext::spawn`] starts an API call on the runtime's async pool
//!    and returns a [`TicketId`] immediately. The rule's invocation ends
//!    without blocking an execution thread.
//! 2. When the operation completes, the runtime re-invokes the rule with a
//!    completion message. The [`entrypoint_with_async!`] macro routes that
//!    message to the continuation handler that was registered with
//!    [`AsyncContext::continue_with`].
//!
//! Because a rule is re-instantiated for every invocation, nothing in the
//! rule's WASM memory survives between steps. State that a continuation
//! needs must be passed through [`AsyncContext::spawn_with_state`] (echoed
//! back by the runtime, capped at a few KiB) or persisted explicitly via
//! `plaid::storage`, the cache, or a shared DB.

use crate::PlaidFunctionError;
use serde::{Deserialize, Serialize};

/// The type of a ticket ID as seen by guest code: 16 raw bytes of an
/// unguessable UUID v4. `Copy` so it can be stored freely.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TicketId(pub [u8; 16]);

impl TicketId {
    /// Parse a ticket ID from its hex string form (`32` hex chars).
    pub fn from_hex(s: &str) -> Result<Self, PlaidFunctionError> {
        let bytes = decode_hex(s).ok_or(PlaidFunctionError::InvalidPointer)?;
        if bytes.len() != 16 {
            return Err(PlaidFunctionError::InvalidPointer);
        }
        let mut id = [0u8; 16];
        id.copy_from_slice(&bytes);
        Ok(Self(id))
    }

    /// Render the ticket ID as a lowercase hex string.
    pub fn to_hex(&self) -> String {
        encode_hex(&self.0)
    }
}

impl std::fmt::Display for TicketId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0xf) as u32, 16).unwrap());
    }
    s
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = (char::from(pair[0])).to_digit(16)?;
        let lo = (char::from(pair[1])).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

/// Status of an outstanding ticket, as reported by `ticket_status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TicketStatus {
    /// The operation is still running.
    Pending,
    /// The operation completed successfully and the result is waiting to be
    /// claimed with `ticket_claim`.
    Completed,
    /// The operation failed. The error string is available via
    /// `ticket_claim`.
    Failed,
    /// The runtime does not know this ticket: it never existed, it was
    /// already claimed, or it expired. Treat this as a failure in cleanup
    /// paths.
    Unknown,
}

/// The outcome of a completed async operation, delivered to a continuation
/// handler.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AsyncResult {
    /// The API call succeeded. `data` holds the return value serialized as
    /// JSON (a string for string-returning calls, a number for numeric ones).
    Ok { data: String },
    /// The API call failed. `error` is a human-readable description.
    Err { error: String },
}

impl AsyncResult {
    /// Interpret the result as a `String` return value. Fails if the
    /// operation failed or the payload is not a JSON string.
    pub fn as_string(self) -> Result<String, PlaidFunctionError> {
        match self {
            AsyncResult::Ok { data } => serde_json::from_str(&data)
                .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize),
            AsyncResult::Err { error } => Err(PlaidFunctionError::InternalApiErrorWith(error)),
        }
    }

    /// Interpret the result as a `u32` return value (status-code style APIs).
    pub fn as_u32(self) -> Result<u32, PlaidFunctionError> {
        match self {
            AsyncResult::Ok { data } => serde_json::from_str(&data)
                .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize),
            AsyncResult::Err { error } => Err(PlaidFunctionError::InternalApiErrorWith(error)),
        }
    }

    /// Interpret the result as a `bool` return value.
    pub fn as_bool(self) -> Result<bool, PlaidFunctionError> {
        match self {
            AsyncResult::Ok { data } => serde_json::from_str(&data)
                .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize),
            AsyncResult::Err { error } => Err(PlaidFunctionError::InternalApiErrorWith(error)),
        }
    }

    /// Interpret the result as an `i32` return value.
    pub fn as_i32(self) -> Result<i32, PlaidFunctionError> {
        match self {
            AsyncResult::Ok { data } => serde_json::from_str(&data)
                .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize),
            AsyncResult::Err { error } => Err(PlaidFunctionError::InternalApiErrorWith(error)),
        }
    }

    /// The error string if the operation failed.
    pub fn error(&self) -> Option<&str> {
        match self {
            AsyncResult::Err { error } => Some(error),
            _ => None,
        }
    }
}

/// Envelope delivered by the runtime when a ticket completes. This is the
/// `data` payload of the completion message; the
/// [`crate::entrypoint_with_async!`] macro deserializes it before calling
/// the continuation handler.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AsyncCompletion {
    /// The ticket that completed.
    pub ticket: TicketId,
    /// Name of the continuation handler the rule registered at spawn time.
    pub continuation: String,
    /// The outcome of the operation.
    pub result: AsyncResult,
    /// The scratchpad state the rule attached at spawn time, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Handle to the async subsystem, passed to `main` and to continuation
/// handlers by the [`crate::entrypoint_with_async!`] macro.
pub struct AsyncContext;

impl AsyncContext {
    /// Spawn an async API call identified by its host function name (e.g.
    /// `"github_add_user_to_repo"`, `"general_make_named_request"`). The
    /// parameters are the same JSON string the synchronous STL function for
    /// that API would take.
    ///
    /// Returns the ticket ID immediately; the call runs on the runtime's
    /// async pool. If `continuation` is non-empty, the rule is re-invoked
    /// with the result when the call completes. If it is empty, the result
    /// can only be retrieved with `ticket_status` / `ticket_claim`.
    ///
    /// `budget` is how many further async spawns the continuation of this
    /// operation will be allowed to perform (the chain budget, mirroring the
    /// logback budget system).
    pub fn spawn(
        &self,
        api_function: &str,
        params: &str,
        continuation: &str,
        budget: u32,
    ) -> Result<TicketId, PlaidFunctionError> {
        Self::spawn_inner(self, api_function, params, continuation, budget, None)
    }

    /// Like [`AsyncContext::spawn`] but attaches a small state payload that
    /// the runtime echoes back in the completion message. Use this to carry
    /// context (e.g. the original request, a correlation ID) into the
    /// continuation without persisting it. The state is capped at 4 KiB.
    pub fn spawn_with_state(
        &self,
        api_function: &str,
        params: &str,
        continuation: &str,
        budget: u32,
        state: &str,
    ) -> Result<TicketId, PlaidFunctionError> {
        Self::spawn_inner(self, api_function, params, continuation, budget, Some(state))
    }

    fn spawn_inner(
        &self,
        api_function: &str,
        params: &str,
        continuation: &str,
        budget: u32,
        state: Option<&str>,
    ) -> Result<TicketId, PlaidFunctionError> {
        extern "C" {
            fn async_spawn(
                api_fn_buf: *const u8,
                api_fn_len: u32,
                params_buf: *const u8,
                params_len: u32,
                continuation_buf: *const u8,
                continuation_len: u32,
                budget: u32,
                state_buf: *const u8,
                state_len: u32,
                ticket_out: *mut u8,
            ) -> i32;
        }

        let api_fn_bytes = api_function.as_bytes().to_vec();
        let params_bytes = params.as_bytes().to_vec();
        let continuation_bytes = continuation.as_bytes().to_vec();
        let state_bytes = state.map(|s| s.as_bytes().to_vec()).unwrap_or_default();

        let mut ticket_out = [0u8; 16];
        let code = unsafe {
            async_spawn(
                api_fn_bytes.as_ptr(),
                api_fn_bytes.len() as u32,
                params_bytes.as_ptr(),
                params_bytes.len() as u32,
                continuation_bytes.as_ptr(),
                continuation_bytes.len() as u32,
                budget,
                state_bytes.as_ptr(),
                state_bytes.len() as u32,
                ticket_out.as_mut_ptr(),
            )
        };

        if code != 0 {
            return Err(code.into());
        }

        Ok(TicketId(ticket_out))
    }

    /// Ask the runtime for the status of a ticket. `Unknown` means the ticket
    /// never existed, was already claimed, or expired.
    pub fn ticket_status(&self, ticket: TicketId) -> Result<TicketStatus, PlaidFunctionError> {
        extern "C" {
            fn ticket_status(ticket: *const u8) -> i32;
        }

        let code = unsafe { ticket_status(ticket.0.as_ptr()) };
        match code {
            0 => Ok(TicketStatus::Pending),
            1 => Ok(TicketStatus::Completed),
            2 => Ok(TicketStatus::Failed),
            3 => Ok(TicketStatus::Unknown),
            e => Err(e.into()),
        }
    }

    /// Claim the result of a completed ticket. This is a one-shot operation:
    /// after a successful claim the ticket is forgotten by the runtime. For
    /// tickets spawned with a continuation this is normally unnecessary
    /// (the result is delivered in the completion message), but it is the
    /// only way to retrieve results of fire-and-forget spawns.
    pub fn ticket_claim(&self, ticket: TicketId) -> Result<AsyncResult, PlaidFunctionError> {
        extern "C" {
            fn ticket_claim(
                ticket: *const u8,
                result_buf: *mut u8,
                result_buf_len: u32,
            ) -> i32;
        }

        // First call with a zero-length buffer to get the required size.
        let needed = unsafe { ticket_claim(ticket.0.as_ptr(), vec![].as_mut_ptr(), 0) };
        if needed < 0 {
            return Err(needed.into());
        }

        let mut buffer = vec![0u8; needed as usize];
        let written = unsafe {
            ticket_claim(
                ticket.0.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
            )
        };
        if written < 0 {
            return Err(written.into());
        }
        buffer.truncate(written as usize);

        serde_json::from_slice(&buffer).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)
    }

    /// Best-effort cancellation of a pending ticket. The underlying API call
    /// may still run to completion; cancellation only stops the runtime from
    /// delivering a completion message.
    pub fn ticket_cancel(&self, ticket: TicketId) -> Result<(), PlaidFunctionError> {
        extern "C" {
            fn ticket_cancel(ticket: *const u8) -> i32;
        }

        let code = unsafe { ticket_cancel(ticket.0.as_ptr()) };
        if code == 0 {
            Ok(())
        } else {
            Err(code.into())
        }
    }
}
