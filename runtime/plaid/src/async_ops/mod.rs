//! The ticket registry: runtime-side tracking of outstanding async
//! operations spawned by rules.
//!
//! A rule cannot yield during its execution (the `wasm32-unknown-unknown`
//! target has no suspension primitives and the host call is synchronous), so
//! "async" is reified as:
//!
//! 1. `async_spawn` allocates a [`TicketId`], spawns the API call on the
//!    shared tokio runtime, and returns the ticket to the guest immediately.
//!    The rule's invocation ends without blocking an execution thread.
//! 2. When the call finishes, the registry stores the result and builds a
//!    completion [`Message`](crate::executor::Message) targeted at the
//!    owning module (via the existing `Message.module` single-module
//!    dispatch path). The rule is re-invoked with the result.
//!
//! The registry also backs the polling API (`ticket_status` /
//! `ticket_claim`) for fire-and-forget spawns and fan-in patterns.

pub mod dispatch;

pub use dispatch::{async_function_allowed_in_test_mode, prepare_async_api_call, SpawnOutcome};

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use plaid_stl::messages::{LogSource, LogbacksAllowed};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::executor::Message;
use crate::loader::PlaidModule;

/// Log type used for async completion messages. This is a reserved internal
/// type: it is never registered in the channel map, so it cannot collide
/// with user log types (same convention as the `logback_internal` storage
/// namespace).
pub const ASYNC_COMPLETION_LOG_TYPE: &str = "__plaid_async";

/// Maximum size of the state payload a rule can attach to a spawn. Mirrors
/// the STL-side limit.
pub const MAX_STATE_BYTES: usize = 4 * 1024;

/// Maximum size of a serialized result kept in the registry. Matches the
/// `Message` data limit.
pub const MAX_RESULT_BYTES: usize = 5 * 1024 * 1024;

/// Default time-to-live for completed-but-unclaimed tickets.
const DEFAULT_COMPLETED_TTL_SECS: u64 = 15 * 60;

/// How often the sweeper task runs.
const SWEEP_INTERVAL_SECS: u64 = 60;

/// A ticket ID: 16 bytes of an unguessable UUID v4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TicketId(pub [u8; 16]);

impl TicketId {
    pub fn new() -> Self {
        Self(*Uuid::new_v4().as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl std::fmt::Display for TicketId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// The outcome of an async API call, as stored in the registry and delivered
/// to rules. Mirrors `plaid_stl::async::AsyncResult` but kept independent so
/// the runtime never depends on guest-side types evolving in lockstep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TicketResult {
    /// The call succeeded; `data` is the return value serialized as JSON.
    Ok { data: String },
    /// The call failed; `error` is a human-readable description.
    Err { error: String },
}

impl TicketResult {
    pub fn ok(data: String) -> Self {
        Self::Ok { data }
    }

    pub fn err(error: impl Into<String>) -> Self {
        Self::Err {
            error: error.into(),
        }
    }
}

/// The envelope delivered to a rule when a ticket completes. Serialized into
/// the completion message's data. This shape must stay in sync with
/// `plaid_stl::async::AsyncCompletion`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AsyncCompletion {
    pub ticket: TicketId,
    pub continuation: String,
    pub result: TicketResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Lifecycle state of a ticket.
enum TicketState {
    /// The API call is in flight.
    Pending,
    /// The call finished; the result is waiting to be claimed or delivered.
    Completed(TicketResult),
}

/// A single tracked async operation.
struct TicketRecord {
    /// The module that spawned the operation (and will be re-invoked).
    owner: String,
    /// Name of the guest continuation handler, empty for fire-and-forget.
    continuation: String,
    /// Scratchpad state to echo back in the completion message.
    state: Option<String>,
    /// Chain budget for the continuation, mirroring the logback budget.
    budget: LogbacksAllowed,
    state_kind: TicketState,
    /// When the ticket was created. Used by the sweeper for logging and by
    /// future durability work; read via `Debug` formatting in diagnostics.
    #[allow(dead_code)]
    created_at: Instant,
    /// Deadline after which a completed-but-unclaimed ticket is dropped.
    expires_at: Option<Instant>,
}

/// Errors returned by registry operations. These map onto the STL's
/// `PlaidFunctionError` codes at the host-function boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketError {
    /// The ticket is unknown: never existed, already claimed, or expired.
    UnknownTicket,
    /// The per-module outstanding-ticket limit was reached.
    TooManyPending,
    /// The state payload exceeded `MAX_STATE_BYTES`.
    StateTooLarge,
    /// The result payload exceeded `MAX_RESULT_BYTES`.
    ResultTooLarge,
}

impl std::fmt::Display for TicketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TicketError::UnknownTicket => write!(f, "Unknown async ticket"),
            TicketError::TooManyPending => write!(f, "Too many pending async tickets"),
            TicketError::StateTooLarge => write!(f, "Async state payload too large"),
            TicketError::ResultTooLarge => write!(f, "Async result too large"),
        }
    }
}

/// Status codes returned by `ticket_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Completed,
    Failed,
    Unknown,
}

/// Configuration for the ticket system.
#[derive(Debug, Clone)]
pub struct TicketConfig {
    /// Maximum outstanding (pending + completed, unclaimed) tickets per
    /// module.
    pub max_tickets_per_module: usize,
    /// Time-to-live for completed-but-unclaimed tickets, in seconds.
    pub completed_ttl_secs: u64,
}

impl Default for TicketConfig {
    fn default() -> Self {
        Self {
            max_tickets_per_module: 64,
            completed_ttl_secs: DEFAULT_COMPLETED_TTL_SECS,
        }
    }
}

impl From<crate::config::AsyncTicketsConfig> for TicketConfig {
    fn from(config: crate::config::AsyncTicketsConfig) -> Self {
        Self {
            max_tickets_per_module: config.max_tickets_per_module,
            completed_ttl_secs: config.completed_ttl_secs,
        }
    }
}

/// The registry itself. Shared between executor threads (spawn/status/
/// claim/cancel are called from host functions) and the tokio tasks that
/// complete the operations.
pub struct TicketRegistry {
    tickets: Mutex<HashMap<TicketId, TicketRecord>>,
    config: TicketConfig,
    /// Total tickets created, for metrics.
    created_total: AtomicU64,
    /// Total tickets completed, for metrics.
    completed_total: AtomicU64,
    /// Total tickets expired by the sweeper, for metrics.
    expired_total: AtomicU64,
}

impl TicketRegistry {
    pub fn new(config: TicketConfig) -> Self {
        Self {
            tickets: Mutex::new(HashMap::new()),
            config,
            created_total: AtomicU64::new(0),
            completed_total: AtomicU64::new(0),
            expired_total: AtomicU64::new(0),
        }
    }

    /// Register a new pending ticket for `owner`. Called by `async_spawn`
    /// before the API call is handed to tokio.
    pub fn insert(
        &self,
        owner: &str,
        continuation: &str,
        state: Option<String>,
        budget: LogbacksAllowed,
    ) -> Result<TicketId, TicketError> {
        if let Some(state) = &state {
            if state.len() > MAX_STATE_BYTES {
                return Err(TicketError::StateTooLarge);
            }
        }

        let mut tickets = self.tickets.lock().map_err(|_| TicketError::UnknownTicket)?;

        // Enforce the per-module outstanding limit. Tickets that already
        // carry a result but have no continuation are unclaimed results:
        // they still count until claimed or expired so a rule cannot hoard
        // the registry.
        let outstanding = tickets
            .values()
            .filter(|t| t.owner == owner)
            .count();
        if outstanding >= self.config.max_tickets_per_module {
            return Err(TicketError::TooManyPending);
        }

        let id = TicketId::new();
        tickets.insert(
            id,
            TicketRecord {
                owner: owner.to_string(),
                continuation: continuation.to_string(),
                state,
                budget,
                state_kind: TicketState::Pending,
                created_at: Instant::now(),
                expires_at: None,
            },
        );
        self.created_total.fetch_add(1, Ordering::Relaxed);

        Ok(id)
    }

    /// Mark a ticket as completed. Returns the completion message to inject
    /// into the executor queue if the ticket has a continuation and is still
    /// pending; `None` if the ticket was cancelled, unknown, or
    /// fire-and-forget (in which case the result simply waits to be
    /// claimed).
    ///
    /// `owner_module` is the module handle captured at spawn time; it is
    /// threaded through by the spawner so the completion can use the
    /// single-module dispatch path.
    pub fn complete(
        &self,
        id: TicketId,
        result: TicketResult,
        owner_module: Arc<PlaidModule>,
    ) -> Option<Message> {
        if let TicketResult::Ok { data } = &result {
            if data.len() > MAX_RESULT_BYTES {
                // Keep the ticket but record the failure: the rule must hear
                // something back rather than hang forever.
                let error = format!(
                    "Async operation result exceeded the {} byte limit",
                    MAX_RESULT_BYTES
                );
                return self.complete(id, TicketResult::err(error), owner_module);
            }
        }

        let mut tickets = self.tickets.lock().ok()?;
        let record = tickets.get_mut(&id)?;

        match record.state_kind {
            TicketState::Pending => {
                record.state_kind = TicketState::Completed(result.clone());
                record.expires_at =
                    Some(Instant::now() + Duration::from_secs(self.config.completed_ttl_secs));
                self.completed_total.fetch_add(1, Ordering::Relaxed);

                if record.continuation.is_empty() {
                    // Fire-and-forget: the result waits for ticket_claim.
                    return None;
                }

                let completion = AsyncCompletion {
                    ticket: id,
                    continuation: record.continuation.clone(),
                    result,
                    state: record.state.clone(),
                };

                let data = serde_json::to_vec(&completion).ok()?;
                let source = LogSource::AsyncCompletion(id.to_hex());

                Some(Message::new_detailed(
                    ASYNC_COMPLETION_LOG_TYPE.to_string(),
                    data,
                    source,
                    record.budget.clone(),
                    Default::default(),
                    None,
                    Some(owner_module),
                ))
            }
            TicketState::Completed(_) => {
                // Double completion (e.g. cancel raced with the task
                // finishing): keep the first result, deliver nothing.
                None
            }
        }
    }

    /// Look up the status of a ticket.
    pub fn status(&self, id: TicketId) -> Status {
        let Ok(tickets) = self.tickets.lock() else {
            return Status::Unknown;
        };

        match tickets.get(&id) {
            None => Status::Unknown,
            Some(record) => match &record.state_kind {
                TicketState::Pending => Status::Pending,
                TicketState::Completed(TicketResult::Ok { .. }) => Status::Completed,
                TicketState::Completed(TicketResult::Err { .. }) => Status::Failed,
            },
        }
    }

    /// Claim the result of a ticket, removing it from the registry. One-shot.
    pub fn claim(&self, id: TicketId) -> Result<TicketResult, TicketError> {
        let mut tickets = self
            .tickets
            .lock()
            .map_err(|_| TicketError::UnknownTicket)?;

        match tickets.remove(&id) {
            Some(record) => match record.state_kind {
                TicketState::Completed(result) => Ok(result),
                // Claiming a pending ticket is not allowed: the operation is
                // still running. The ticket stays registered.
                TicketState::Pending => {
                    tickets.insert(id, record);
                    Err(TicketError::UnknownTicket)
                }
            },
            None => Err(TicketError::UnknownTicket),
        }
    }

    /// Best-effort cancellation: forget the ticket so no completion is
    /// delivered. The underlying API call may still run to completion; its
    /// result is simply discarded.
    pub fn cancel(&self, id: TicketId) -> Result<(), TicketError> {
        let mut tickets = self
            .tickets
            .lock()
            .map_err(|_| TicketError::UnknownTicket)?;

        match tickets.remove(&id) {
            Some(_) => Ok(()),
            None => Err(TicketError::UnknownTicket),
        }
    }

    /// Drop completed tickets whose TTL has elapsed. Returns the number of
    /// expired tickets. Called periodically by the sweeper task.
    pub fn sweep_expired(&self) -> usize {
        let Ok(mut tickets) = self.tickets.lock() else {
            return 0;
        };

        let now = Instant::now();
        let before = tickets.len();
        tickets.retain(|_, record| match &record.expires_at {
            Some(deadline) => now < *deadline,
            None => true,
        });
        let expired = before - tickets.len();
        if expired > 0 {
            self.expired_total.fetch_add(expired as u64, Ordering::Relaxed);
        }
        expired
    }

    /// Number of tickets currently tracked (pending + completed unclaimed).
    pub fn outstanding(&self) -> usize {
        self.tickets.lock().map(|t| t.len()).unwrap_or(0)
    }

    /// Total tickets created since startup.
    pub fn created_count(&self) -> u64 {
        self.created_total.load(Ordering::Relaxed)
    }

    /// Total tickets completed since startup.
    pub fn completed_count(&self) -> u64 {
        self.completed_total.load(Ordering::Relaxed)
    }

    /// Total tickets expired by the sweeper since startup.
    pub fn expired_count(&self) -> u64 {
        self.expired_total.load(Ordering::Relaxed)
    }
}

/// Spawn the background sweeper that evicts completed-but-unclaimed tickets
/// once their TTL elapses. Returns the `JoinHandle` so the caller can join
/// it during shutdown.
pub fn spawn_sweeper(
    registry: Arc<TicketRegistry>,
    cancellation_token: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let interval = Duration::from_secs(SWEEP_INTERVAL_SECS);
        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    return;
                }
                _ = tokio::time::sleep(interval) => {
                    let expired = registry.sweep_expired();
                    if expired > 0 {
                        log::warn!("Expired {expired} unclaimed async ticket(s)");
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> TicketRegistry {
        TicketRegistry::new(TicketConfig::default())
    }

    #[test]
    fn ticket_ids_are_unique() {
        let a = TicketId::new();
        let b = TicketId::new();
        assert_ne!(a, b);
        assert_eq!(a.to_hex().len(), 32);
    }

    #[test]
    fn insert_and_status_roundtrip() {
        let reg = registry();
        let id = reg
            .insert("m.wasm", "on_done", None, LogbacksAllowed::Limited(1))
            .unwrap();
        assert_eq!(reg.status(id), Status::Pending);
    }

    #[test]
    fn per_module_limit_is_enforced() {
        let config = TicketConfig {
            max_tickets_per_module: 2,
            ..Default::default()
        };
        let reg = TicketRegistry::new(config);
        assert!(reg
            .insert("m.wasm", "", None, LogbacksAllowed::Limited(0))
            .is_ok());
        assert!(reg
            .insert("m.wasm", "", None, LogbacksAllowed::Limited(0))
            .is_ok());
        assert_eq!(
            reg.insert("m.wasm", "", None, LogbacksAllowed::Limited(0)),
            Err(TicketError::TooManyPending)
        );
        // Other modules are unaffected.
        assert!(reg
            .insert("other.wasm", "", None, LogbacksAllowed::Limited(0))
            .is_ok());
    }

    #[test]
    fn state_size_limit_is_enforced() {
        let reg = registry();
        let big = "x".repeat(MAX_STATE_BYTES + 1);
        assert_eq!(
            reg.insert("m.wasm", "", Some(big), LogbacksAllowed::Limited(0)),
            Err(TicketError::StateTooLarge)
        );
    }

    #[test]
    fn claim_removes_ticket() {
        let reg = registry();
        let id = reg
            .insert("m.wasm", "", None, LogbacksAllowed::Limited(0))
            .unwrap();
        // Pending tickets cannot be claimed.
        assert_eq!(reg.claim(id), Err(TicketError::UnknownTicket));
        assert_eq!(reg.status(id), Status::Pending);

        // Fire-and-forget completion (no continuation): no message produced.
        assert!(reg
            .complete(id, TicketResult::ok("42".into()), test_module())
            .is_none());
        assert_eq!(reg.status(id), Status::Completed);

        let result = reg.claim(id).unwrap();
        assert!(matches!(result, TicketResult::Ok { .. }));
        // Claim is one-shot.
        assert_eq!(reg.status(id), Status::Unknown);
        assert_eq!(reg.claim(id), Err(TicketError::UnknownTicket));
    }

    #[test]
    fn completion_with_continuation_builds_message() {
        let reg = registry();
        let id = reg
            .insert(
                "m.wasm",
                "on_done",
                Some("{\"k\":1}".to_string()),
                LogbacksAllowed::Limited(2),
            )
            .unwrap();

        let message = reg
            .complete(id, TicketResult::ok("\"hello\"".into()), test_module())
            .expect("completion message should be produced");

        assert_eq!(message.type_, ASYNC_COMPLETION_LOG_TYPE);
        assert!(matches!(message.source, LogSource::AsyncCompletion(_)));
        assert!(message.module.is_some());

        let completion: AsyncCompletion = serde_json::from_slice(&message.data).unwrap();
        assert_eq!(completion.continuation, "on_done");
        assert_eq!(completion.state.as_deref(), Some("{\"k\":1}"));
        assert!(matches!(completion.result, TicketResult::Ok { .. }));
    }

    #[test]
    fn oversize_results_are_recorded_as_failures() {
        let reg = registry();
        let id = reg
            .insert("m.wasm", "on_done", None, LogbacksAllowed::Limited(0))
            .unwrap();
        let huge = "x".repeat(MAX_RESULT_BYTES + 1);
        let message = reg
            .complete(id, TicketResult::ok(huge), test_module())
            .expect("failure completion should still be delivered");
        let completion: AsyncCompletion = serde_json::from_slice(&message.data).unwrap();
        assert!(matches!(completion.result, TicketResult::Err { .. }));
    }

    #[test]
    fn cancel_forgets_ticket() {
        let reg = registry();
        let id = reg
            .insert("m.wasm", "on_done", None, LogbacksAllowed::Limited(0))
            .unwrap();
        assert!(reg.cancel(id).is_ok());
        assert_eq!(reg.status(id), Status::Unknown);
        assert_eq!(reg.cancel(id), Err(TicketError::UnknownTicket));
    }

    #[test]
    fn sweep_drops_expired_completed_tickets() {
        let config = TicketConfig {
            completed_ttl_secs: 0,
            ..Default::default()
        };
        let reg = TicketRegistry::new(config);
        let id = reg
            .insert("m.wasm", "", None, LogbacksAllowed::Limited(0))
            .unwrap();
        reg.complete(id, TicketResult::ok("1".into()), test_module());
        // TTL of 0 means the deadline is already in the past.
        assert_eq!(reg.sweep_expired(), 1);
        assert_eq!(reg.status(id), Status::Unknown);
    }

    #[test]
    fn double_completion_delivers_once() {
        let reg = registry();
        let id = reg
            .insert("m.wasm", "on_done", None, LogbacksAllowed::Limited(0))
            .unwrap();
        let module = test_module();
        assert!(reg
            .complete(id, TicketResult::ok("1".into()), module.clone())
            .is_some());
        // Second completion (e.g. cancel raced with the task) is a no-op.
        assert!(reg
            .complete(id, TicketResult::ok("2".into()), module)
            .is_none());
        // The first result is what a claim sees.
        assert!(matches!(reg.claim(id), Ok(TicketResult::Ok { .. })));
    }

    /// A minimal `PlaidModule` for registry tests. The registry only reads
    /// `name`/`logtype` when building completion messages, so a stub module
    /// compiled from an empty wasm binary is sufficient.
    fn test_module() -> Arc<PlaidModule> {
        crate::tests::stub_module("m.wasm", "m")
    }
}
