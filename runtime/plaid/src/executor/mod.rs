pub mod metrics;
pub mod thread_pools;

use crate::apis::Api;

use crate::cache::Cache;
use crate::data::DelayedMessage;
use crate::functions::{
    create_bindgen_externref_xform, create_bindgen_placeholder, link_functions_to_module, LinkError,
};
use crate::loader::PlaidModule;
use crate::logging::{Logger, LoggingError, Severity};
use crate::performance::ModulePerformanceMetadata;
use crate::storage::Storage;

use crossbeam_channel::{Receiver, RecvError, Sender, TrySendError};
use metrics::ModuleExecutionMetrics;
pub use thread_pools::{ExecutionThreadPools, MessageSender};
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::oneshot::Sender as OneShotSender;
use tokio_util::sync::CancellationToken;

use plaid_stl::messages::{LogSource, LogbacksAllowed};
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{serde_as, DeserializeAs, SerializeAs};
use wasmer::{
    AsStoreAsync, FunctionEnv, Imports, Instance, Memory, RuntimeError, Store, StoreAsync,
    TypedFunction,
};
use wasmer_middlewares::metering::{get_remaining_points, MeteringPoints};

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// When a rule is used to generate a webhook response, this structure is what
/// is passed from the executor to the async webhook runtime.
#[derive(Serialize, Deserialize)]
pub struct ResponseMessage {
    /// The HTTP status selected by the response rule.
    pub code: u16,
    /// The data the rule intends to return in the serviced webhook request.
    pub body: String,
}

const MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_ENTRIES: usize = 20;
const MAX_KEY_LEN: usize = 100;

// ---- small adapters ----

struct VecMax<const MAX: usize>;

impl SerializeAs<Vec<u8>> for VecMax<MAX_BYTES> {
    fn serialize_as<S>(value: &Vec<u8>, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        s.collect_seq(value)
    }
}

impl<'de> DeserializeAs<'de, Vec<u8>> for VecMax<MAX_BYTES> {
    fn deserialize_as<D>(d: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Vec::<u8>::deserialize(d)?;
        if v.len() > MAX_BYTES {
            return Err(serde::de::Error::custom("Vec<u8> exceeds the size limit"));
        }
        Ok(v)
    }
}

// Enforce map entry count + key length, while using VecMax for values.
fn map_with_limits<'de, D>(de: D) -> Result<HashMap<String, Vec<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    // Deserialize values via the VecMax adapter
    #[serde_as]
    #[derive(Deserialize)]
    struct Tmp(#[serde_as(as = "HashMap<_, VecMax<MAX_BYTES>>")] HashMap<String, Vec<u8>>);

    let Tmp(map) = Tmp::deserialize(de)?;
    if map.len() > MAX_ENTRIES {
        return Err(serde::de::Error::custom(&format!(
            "map exceeds {MAX_ENTRIES} entries"
        )));
    }
    for k in map.keys() {
        if k.chars().count() > MAX_KEY_LEN {
            return Err(serde::de::Error::custom(&format!(
                "key exceeds {MAX_KEY_LEN} chars"
            )));
        }
    }
    Ok(map)
}

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub type_: String,

    /// <= 5 MiB
    #[serde_as(as = "VecMax<MAX_BYTES>")]
    pub data: Vec<u8>,

    /// <= 20 entries; key <= 100 chars; value <= 5 MiB
    #[serde(deserialize_with = "map_with_limits", default)]
    pub headers: HashMap<String, Vec<u8>>,

    /// <= 20 entries; key <= 100 chars; value <= 5 MiB
    #[serde(deserialize_with = "map_with_limits", default)]
    pub query_params: HashMap<String, Vec<u8>>,

    /// Where the message came from
    pub source: LogSource,
    pub logbacks_allowed: LogbacksAllowed,
    /// If a response is should be sent back to the source of the message
    /// This is used in the GET request system to handle responses
    #[serde(skip)]
    pub response_sender: Option<OneShotSender<Option<ResponseMessage>>>,
    /// If this is some, the entire channel will not be run, just a specific
    /// module. This is used in the GET system because only one rule can
    /// be run to generate a response.
    #[serde(skip)]
    pub module: Option<Arc<PlaidModule>>,
}

impl Message {
    pub fn new(
        type_: String,
        data: Vec<u8>,
        source: LogSource,
        logbacks_allowed: LogbacksAllowed,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            type_,
            data,
            headers: HashMap::new(),
            query_params: HashMap::new(),
            source,
            logbacks_allowed,
            response_sender: None,
            module: None,
        }
    }

    /// Construct a new message with optional fields
    pub fn new_detailed(
        type_: String,
        data: Vec<u8>,
        source: LogSource,
        logbacks_allowed: LogbacksAllowed,
        query_params: HashMap<String, Vec<u8>>,
        response_sender: Option<OneShotSender<Option<ResponseMessage>>>,
        module: Option<Arc<PlaidModule>>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            type_,
            data,
            headers: HashMap::new(),
            query_params,
            source,
            logbacks_allowed,
            response_sender,
            module,
        }
    }

    /// Create a duplicate of the message that does
    /// not have the response sender.
    pub fn create_duplicate(&self) -> Self {
        Self {
            id: self.id.clone(),
            type_: self.type_.clone(),
            data: self.data.clone(),
            headers: self.headers.clone(),
            query_params: self.query_params.clone(),
            source: self.source.clone(),
            logbacks_allowed: self.logbacks_allowed.clone(),
            response_sender: None,
            module: None,
        }
    }
}

/// Environment for executing a module on a message
pub struct Env {
    // A handle to the module which is processing the message
    pub module: Arc<PlaidModule>,
    // The message that is being processed.
    pub message: Message,
    // A handle to the API to make external calls
    pub api: Arc<Api>,
    // A handle to the storage system if one is configured
    pub storage: Option<Arc<Storage>>,
    // A handle to the cache system if one is configured
    pub cache: Option<Arc<Cache>>,
    // A sender to the external logging system
    pub external_logging_system: Logger,
    /// Memory for host-guest communication
    pub memory: Option<Memory>,
    // A special value that can be filled to leave a string response available after
    // the module has executed. Generally this is used for webhook responses.
    pub response: Option<String>,
    /// The HTTP status selected by a response rule.
    pub response_status: Option<u16>,
    /// An invalid status supplied by a rule. This turns the invocation into a
    /// response failure even when the rule ignores the host function result.
    pub invalid_response_status: Option<u32>,
    // Context about error encountered by the module during its execution
    pub execution_error_context: Option<String>,
    /// Available for immediate logback during normal operation; `None` during shutdown.
    /// Routes messages to the pool dedicated to their log type, when one exists.
    pub immediate_sender: Option<MessageSender>,
    /// Sender for delayed logbacks (`delay > 0`), and for immediate logbacks coerced
    /// during shutdown. Messages are persisted by the internal logback listener and
    /// injected into the executor queue once their delay elapses.
    pub delayed_log_sender: Sender<DelayedMessage>,
    /// Shared with async tasks; set when shutdown begins.
    pub cancellation_token: CancellationToken,
}

/// The executor that processes messages
pub struct Executor {
    /// Routes every inbound message to the pool dedicated to its log type,
    /// when one exists, or to the general pool otherwise.
    message_sender: MessageSender,
}

/// Join handles for executor worker threads.
pub struct ExecutorThreads {
    thread_handles: Vec<JoinHandle<()>>,
}

impl ExecutorThreads {
    /// Wait for worker threads to exit after all executor ingress senders have been dropped.
    pub fn join(self) {
        for handle in self.thread_handles {
            if let Err(e) = handle.join() {
                error!("Execution thread panicked during shutdown: {e:?}");
            }
        }
    }
}

/// Errors encountered by the executor while trying to execute a module
pub enum ExecutorError {
    ExternalLoggingError(LoggingError),
    LinkError(LinkError),
    InstantiationError(String),
    MemoryError(String),
    NoEntrypoint,
    InvalidEntrypoint,
    ModuleExecutionError(ModuleExecutionError),
    IncomingLogError(RecvError),
}

impl std::fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutorError::ExternalLoggingError(e) => write!(f, "External Logging Error: {e}"),
            ExecutorError::LinkError(e) => write!(f, "Link Error: {e}"),
            ExecutorError::InstantiationError(e) => write!(f, "Instantiation Error: {e}"),
            ExecutorError::MemoryError(e) => write!(f, "Memory Error: {e}"),
            ExecutorError::NoEntrypoint => write!(f, "No entrypoint found in module"),
            ExecutorError::InvalidEntrypoint => write!(
                f,
                "Entrypoint is not a function or not the correct prototype"
            ),
            ExecutorError::ModuleExecutionError(e) => write!(f, "Module Execution Error: {e}"),
            ExecutorError::IncomingLogError(e) => write!(f, "Error reading log from channel: {e}"),
        }
    }
}

/// Error encountered during the execution of a module
pub enum ModuleExecutionError {
    ComputationExhausted(u64),
    ModuleError(String),
    PersistentResponseNotAllowed,
    PersistentResponseTooLarge {
        max_size: usize,
        response_size: usize,
    },
    LockingError(String),
    UnknownExecutionError(String),
}

impl Into<ExecutorError> for ModuleExecutionError {
    fn into(self) -> ExecutorError {
        ExecutorError::ModuleExecutionError(self)
    }
}

impl std::fmt::Display for ModuleExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleExecutionError::ComputationExhausted(limit) => {
                write!(f, "Computation Exhausted. Limit: [{limit}]")
            }
            ModuleExecutionError::ModuleError(context) => {
                write!(
                    f,
                    "Module encountered an error. Additional context: [{context}]"
                )
            }
            ModuleExecutionError::UnknownExecutionError(error) => {
                write!(f, "Unknown execution error. Error: [{error}]")
            }
            ModuleExecutionError::PersistentResponseNotAllowed => {
                write!(f, "Persistent response not allowed")
            }
            ModuleExecutionError::PersistentResponseTooLarge {
                max_size,
                response_size,
            } => {
                write!(f, "Persistent response too large. Max size: [{max_size}], Response size: [{response_size}]")
            }
            ModuleExecutionError::LockingError(error) => {
                write!(f, "CRITICAL Locking error. Error: [{error}]")
            }
        }
    }
}

impl From<LoggingError> for ExecutorError {
    fn from(e: LoggingError) -> Self {
        ExecutorError::ExternalLoggingError(e)
    }
}

/// Everything needed to run a module on a message via `call_async`.
///
/// `Store::into_async()` consumes the `Store`, so the bundle holds only the
/// `StoreAsync` handle. All post-call access to the store (response reads,
/// metering) goes through its read/write locks.
pub struct PreparedExecution {
    pub store_async: StoreAsync,
    pub instance: Instance,
    pub entrypoint: TypedFunction<(), i32>,
    pub env: FunctionEnv<Env>,
}

/// Take a message, a module, and an executor and get an instance back that is ready to run
/// the provided module.
fn prepare_for_execution(
    message: Message,
    plaid_module: Arc<PlaidModule>,
    api: Arc<Api>,
    storage: Option<Arc<Storage>>,
    cache: Option<Arc<Cache>>,
    els: Logger,
    response: Option<String>,
    immediate_sender: Option<MessageSender>,
    delayed_log_sender: Sender<DelayedMessage>,
    cancellation_token: CancellationToken,
) -> Result<PreparedExecution, ExecutorError> {
    // Prepare the structure for functions the module will use
    // AKA: Host Functions
    let mut imports = Imports::new();

    // Create the store we're going to use to execute the module
    // for this message only.
    let mut store = Store::new(plaid_module.engine.clone());

    let env = Env {
        module: plaid_module.clone(),
        message: message.create_duplicate(),
        api: api.clone(),
        storage: storage.clone(),
        cache: cache.clone(),
        external_logging_system: els.clone(),
        memory: None,
        response,
        response_status: None,
        invalid_response_status: None,
        execution_error_context: None,
        immediate_sender,
        delayed_log_sender,
        cancellation_token,
    };

    let env = FunctionEnv::new(&mut store, env);

    let exports = match link_functions_to_module(&plaid_module.module, &mut store, env.clone()) {
        Ok(exports) => exports,
        Err(e) => {
            els.log_module_error(
                plaid_module.name.clone(),
                format!("Failed to link functions to module: {:?}", e),
                message.data.clone(),
            )?;
            return Err(ExecutorError::LinkError(e));
        }
    };

    // Set up the environment
    imports.register_namespace("env", exports);
    imports.register_namespace(
        "__wbindgen_placeholder__",
        create_bindgen_placeholder(&plaid_module.module, &mut store),
    );
    imports.register_namespace(
        "__wbindgen_externref_xform__",
        create_bindgen_externref_xform(&mut store),
    );
    let instance = match Instance::new(&mut store, &plaid_module.module, &imports) {
        Ok(i) => i,
        Err(e) => {
            els.log_module_error(
                plaid_module.name.clone(),
                format!("Failed to instantiate module: {e}"),
                message.data.clone(),
            )?;
            return Err(ExecutorError::InstantiationError(e.to_string()));
        }
    };

    // We have to give the function environment a reference to the memory
    // that it can use for communication with the module
    let mut env_mut = env.into_mut(&mut store);
    let data_mut = env_mut.data_mut();
    data_mut.memory = match instance.exports.get_memory("memory") {
        Ok(memory) => Some(memory.clone()),
        Err(e) => {
            els.log_module_error(
                plaid_module.name.clone(),
                format!("Failed to get memory from module: {e}"),
                message.data.clone(),
            )?;
            return Err(ExecutorError::MemoryError(e.to_string()));
        }
    };

    let envr = env_mut.as_ref();
    // Get the entrypoint of the module
    let ep = instance
        .exports
        .get_function("entrypoint")
        .map_err(|_| ExecutorError::NoEntrypoint)?
        .typed::<(), i32>(&mut store)
        .map_err(|_| ExecutorError::InvalidEntrypoint)?;

    // Convert the store into its async handle. This consumes the store:
    // from here on, all access goes through StoreAsync's read/write locks.
    let store_async = store.into_async();

    Ok(PreparedExecution {
        store_async,
        instance,
        entrypoint: ep,
        env: envr,
    })
}

/// Update a module's persistent response
async fn update_persistent_response(
    plaid_module: &Arc<PlaidModule>,
    env: &FunctionEnv<Env>,
    store_async: &StoreAsync,
) -> Result<(), ExecutorError> {
    let response = {
        let lock = store_async.read_lock().await;
        env.as_ref(&lock).response.clone()
    };
    match (response, &plaid_module.persistent_response) {
        (None, _) => {
            // There was no response to save
            return Ok(());
        }
        (Some(_), None) => {
            warn!(
                "{} tried to set a persistent response but it is not allowed to do so",
                plaid_module.name
            );
            return Ok(());
        }
        (Some(response), Some(pr)) => {
            // Check to see if the response size is within limits
            if response.len() <= pr.max_size {
                match pr.data.write() {
                    Ok(mut data) => {
                        *data = Some(response.clone());
                        info!("{} updated its persistent response", plaid_module.name);
                        Ok(())
                    }
                    Err(e) => Err(ModuleExecutionError::LockingError(format!("{e}")).into()),
                }
            } else {
                Err(ModuleExecutionError::PersistentResponseTooLarge {
                    max_size: pr.max_size,
                    response_size: response.len(),
                }
                .into())
            }
        }
    }
}

/// This runs a message through a module and will handle module level errors.
///
/// If there is a runtime level error then this function returns an error which
/// will stop Plaid. This means that a module should NEVER be able to cause such
/// an error. The only time this should return an error is if the runtime itself
/// encounters a critical, unrecoverable error.
async fn process_message_with_module(
    message: Message,
    module: Arc<PlaidModule>,
    api: Arc<Api>,
    storage: Option<Arc<Storage>>,
    cache: Option<Arc<Cache>>,
    els: Logger,
    performance_mode: Option<Sender<ModulePerformanceMetadata>>,
    module_execution_metrics: Option<Arc<ModuleExecutionMetrics>>,
    immediate_sender: Option<MessageSender>,
    delayed_log_sender: Sender<DelayedMessage>,
    cancellation_token: CancellationToken,
) -> Result<(), ExecutorError> {
    // TODO @obelisk: This will quietly swallow locking errors on the persistent response
    // This will eventually be caught if something tries to update the response but I don't
    // know if that's good enough.
    let persistent_response = module.get_persistent_response_data();
    // Message needs to be cloned because of the logback budget
    // which is separate for every rule running the same message.
    let prepared = match prepare_for_execution(
        message.create_duplicate(),
        module.clone(),
        api.clone(),
        storage.clone(),
        cache.clone(),
        els.clone(),
        persistent_response,
        immediate_sender,
        delayed_log_sender,
        cancellation_token,
    ) {
        Ok(prepared) => prepared,
        Err(e) => {
            els.log_module_error(
                module.name.clone(),
                format!("Failed to prepare for execution: {e}"),
                message.data.clone(),
            )?;
            return Ok(());
        }
    };

    let computation_limit = module.computation_limit;
    // Call the entrypoint via call_async: this is the async boundary. The
    // guest's stack parks inside async host imports until their futures
    // complete, and the executor thread is free while it is suspended.
    let begin = Instant::now();
    let error = match prepared.entrypoint.call_async(&prepared.store_async).await {
        Ok(n) => {
            if n != 0 {
                if let Some(metrics) = &module_execution_metrics {
                    metrics.record_module_failure(&module.name);
                }

                let error_context = {
                    let lock = prepared.store_async.read_lock().await;
                    prepared
                        .env
                        .as_ref(&lock)
                        .execution_error_context
                        .clone()
                        .unwrap_or("None".to_string())
                };
                Some(ModuleExecutionError::ModuleError(error_context))
            } else {
                // This should always work because when computation is exhausted,
                // we end up in the RuntimeError block.
                let remaining = {
                    let mut lock = prepared.store_async.write_lock().await;
                    match get_remaining_points(&mut lock, &prepared.instance) {
                        MeteringPoints::Remaining(remaining) => remaining,
                        MeteringPoints::Exhausted => 0,
                    }
                };

                let computation_remaining_percentage =
                    (remaining as f32 / computation_limit as f32) * 100.0;
                let computation_used = 100.0 - computation_remaining_percentage;

                if let Some(metrics) = &module_execution_metrics {
                    metrics.record_successful_execution(
                        &module.name,
                        computation_used as f64,
                        begin.elapsed(),
                    );
                }

                // If performance monitoring is enabled, log data to the monitoring system
                if let Some(ref sender) = performance_mode {
                    if let Err(e) = sender.send(ModulePerformanceMetadata {
                        module: module.name.clone(),
                        execution_time: begin.elapsed().as_micros(),
                        computation_used: computation_limit - remaining,
                    }) {
                        error!("Failed to send rule execution metadata to performance monitoring system for {}. Error: {e}", module.name)
                    }
                }

                None
            }
        }
        Err(e) => Some(
            determine_error(
                e,
                computation_limit,
                &prepared.instance,
                &prepared.store_async,
                &prepared.env,
            )
            .await,
        ),
    };

    // If there was an error then log that it happened to the els
    if let Some(error) = error {
        els.log_module_error(
            module.name.clone(),
            format!("{error}"),
            message.data.clone(),
        )?;

        // Stop processing this log and move on to the next one
        return Ok(());
    }

    // Check to see if there is data in the error context even if the module didn't report an error
    // Modules can do this to return warnings it wants to surface without affecting error metrics
    let return_message = {
        let lock = prepared.store_async.read_lock().await;
        prepared.env.as_ref(&lock).execution_error_context.clone()
    };
    if let Some(return_message) = &return_message {
        let _ = els.log_internal_message(
            Severity::Info,
            format!("Module [{}] returned: {}", module.name, return_message),
        );
    }

    let (invalid_status, response, response_status) = {
        let lock = prepared.store_async.read_lock().await;
        let env_ref = prepared.env.as_ref(&lock);
        (
            env_ref.invalid_response_status,
            env_ref.response.clone(),
            env_ref.response_status,
        )
    };

    if let Some(invalid_status) = invalid_status {
        if let Some(sender) = message.response_sender {
            let _ = sender.send(None);
        }
        els.log_module_error(
            module.name.clone(),
            format!("Invalid HTTP response status: {invalid_status}"),
            message.data.clone(),
        )?;
        return Ok(());
    }

    if let Some(sender) = message.response_sender {
        let response = response.map(|body| ResponseMessage {
            code: response_status.unwrap_or(200),
            body,
        });
        if sender.send(response).is_err() {
            error!(
                "[{}] was servicing a request but sending the response failed!",
                module.name
            );
        }
    }

    // Update the persistent response
    if let Err(e) = update_persistent_response(&module, &prepared.env, &prepared.store_async).await
    {
        let _ = els.log_module_error(
            module.name.clone(),
            format!("Failed to update persistent response: {e}"),
            message.data.clone(),
        );
    }

    Ok(())
}

fn execution_loop(
    receiver: Receiver<Message>,
    modules: HashMap<String, Vec<Arc<PlaidModule>>>,
    api: Arc<Api>,
    storage: Option<Arc<Storage>>,
    cache: Option<Arc<Cache>>,
    els: Logger,
    performance_monitoring_mode: Option<Sender<ModulePerformanceMetadata>>,
    module_execution_metrics: Option<Arc<ModuleExecutionMetrics>>,
    immediate_sender: Weak<MessageSender>,
    delayed_log_sender: Sender<DelayedMessage>,
    cancellation_token: CancellationToken,
    runtime_handle: TokioRuntimeHandle,
) -> Result<(), ExecutorError> {
    // Worker threads drive each message with `runtime_handle.block_on(...)`
    // on the process-wide tokio runtime (the one `#[tokio::main]` creates and
    // that `Api`'s reqwest clients were built on).
    //
    // `Handle::block_on` polls the future on the *calling* thread, which is
    // what wasmer's `call_async` requires: the guest's coroutine stack is
    // thread-local (corosensei), so the entrypoint must be resumed on the
    // thread that started it. Timers and I/O tasks the future spawns are
    // driven by the shared runtime's worker threads.
    //
    // Why not a per-thread current-thread runtime: the reqwest clients held
    // by `Api` are shared across all executor threads. A pooled connection's
    // driver task lives on whichever runtime first established it. If each
    // executor thread had its own runtime, a thread parked in `recv()` would
    // never drive the connection another thread was trying to reuse, and
    // that request would stall until the client timeout (observed as 5s
    // `TimedOut`s in the cron integration test). Sharing one runtime keeps
    // every connection driver on always-driven worker threads.
    //
    // SAFETY INVARIANT: never `runtime_handle.spawn` work that must complete
    // for drain correctness. The executor only `block_on`s, so a worker
    // thread only ever exits between messages, never mid-message — drain
    // semantics are preserved.

    loop {
        let message = match receiver.recv() {
            Ok(message) => message,
            Err(RecvError) => return Ok(()),
        };

        let immediate_sender = if cancellation_token.is_cancelled() {
            None
        } else {
            immediate_sender.upgrade().map(|sender| (*sender).clone())
        };

        // Check that we know what modules to send this new log to
        match (&message.module, modules.get(&message.type_)) {
            // If this message has a response sender, we only
            // want to run it on that rule, not any defined logging
            // channel.
            (Some(ref module), _) => {
                let module = module.clone();
                runtime_handle.block_on(process_message_with_module(
                    message,
                    module,
                    api.clone(),
                    storage.clone(),
                    cache.clone(),
                    els.clone(),
                    performance_monitoring_mode.clone(),
                    module_execution_metrics.clone(),
                    immediate_sender.clone(),
                    delayed_log_sender.clone(),
                    cancellation_token.clone(),
                ))?;
            }
            (None, Some(modules)) => {
                // For every module that operates on that log type
                for module in modules {
                    runtime_handle.block_on(process_message_with_module(
                        message.create_duplicate(),
                        module.clone(),
                        api.clone(),
                        storage.clone(),
                        cache.clone(),
                        els.clone(),
                        performance_monitoring_mode.clone(),
                        module_execution_metrics.clone(),
                        immediate_sender.clone(),
                        delayed_log_sender.clone(),
                        cancellation_token.clone(),
                    ))?;
                }
            }
            (None, None) => {
                warn!(
                    "Got logs of a type we have no modules for? Type was: {}",
                    message.type_
                );
                continue;
            }
        };
    }
}

async fn determine_error(
    e: RuntimeError,
    computation_limit: u64,
    instance: &Instance,
    store_async: &StoreAsync,
    env: &FunctionEnv<Env>,
) -> ModuleExecutionError {
    // First check to see if we've exhausted computation
    let exhausted = {
        let mut lock = store_async.write_lock().await;
        matches!(
            get_remaining_points(&mut lock, instance),
            MeteringPoints::Exhausted
        )
    };
    if exhausted {
        return ModuleExecutionError::ComputationExhausted(computation_limit);
    }

    // If all else fails, it's an unknown error
    let context = {
        let lock = store_async.read_lock().await;
        env.as_ref(&lock)
            .execution_error_context
            .clone()
            .unwrap_or("This is probably an OOM error".to_string())
    };
    ModuleExecutionError::UnknownExecutionError(format!("{e}. Additional context: {context}"))
}

impl Executor {
    pub fn new(
        thread_pools: ExecutionThreadPools,
        modules: HashMap<String, Vec<Arc<PlaidModule>>>,
        api: Arc<Api>,
        storage: Option<Arc<Storage>>,
        cache: Option<Arc<Cache>>,
        els: Logger,
        performance_monitoring_mode: Option<Sender<ModulePerformanceMetadata>>,
        module_execution_metrics: Option<Arc<ModuleExecutionMetrics>>,
        immediate_sender: Weak<MessageSender>,
        delayed_log_sender: Sender<DelayedMessage>,
        cancellation_token: CancellationToken,
        runtime_handle: TokioRuntimeHandle,
    ) -> (Self, ExecutorThreads) {
        let mut thread_handles = Vec::new();

        // General processing
        for i in 0..thread_pools.general_pool.num_threads {
            info!("Starting Execution Thread {i} Dedicated to General Processing");
            let receiver = thread_pools.general_pool.receiver.clone();
            let api = api.clone();
            let storage = storage.clone();
            let cache = cache.clone();
            let modules = modules.clone();
            let els = els.clone();
            let performance_sender = performance_monitoring_mode.clone();
            let module_execution_metrics = module_execution_metrics.clone();
            let immediate_sender = immediate_sender.clone();
            let delayed_log_sender = delayed_log_sender.clone();
            let cancellation_token = cancellation_token.clone();
            let runtime_handle = runtime_handle.clone();
            let handle = thread::spawn(move || {
                if let Err(e) = execution_loop(
                    receiver.clone(),
                    modules.clone(),
                    api.clone(),
                    storage.clone(),
                    cache.clone(),
                    els.clone(),
                    performance_sender.clone(),
                    module_execution_metrics.clone(),
                    immediate_sender.clone(),
                    delayed_log_sender.clone(),
                    cancellation_token.clone(),
                    runtime_handle.clone(),
                ) {
                    error!("General execution thread {i} exited with error: {e}");
                }
            });
            thread_handles.push(handle);
        }

        // Dedicated processing
        for (log_type, thread_pool) in &thread_pools.dedicated_pools {
            for i in 0..thread_pool.num_threads {
                info!("Starting Execution Thread {i} Dedicated to {log_type}");
                let receiver = thread_pool.receiver.clone();
                let api = api.clone();
                let storage = storage.clone();
                let cache = cache.clone();
                let modules = modules.clone();
                let els = els.clone();
                let performance_sender = performance_monitoring_mode.clone();
                let module_execution_metrics = module_execution_metrics.clone();
                let log_type = log_type.clone();
                let immediate_sender = immediate_sender.clone();
                let delayed_log_sender = delayed_log_sender.clone();
                let cancellation_token = cancellation_token.clone();
                let runtime_handle = runtime_handle.clone();
                let handle = thread::spawn(move || {
                    if let Err(e) = execution_loop(
                        receiver.clone(),
                        modules.clone(),
                        api.clone(),
                        storage.clone(),
                        cache.clone(),
                        els.clone(),
                        performance_sender.clone(),
                        module_execution_metrics.clone(),
                        immediate_sender.clone(),
                        delayed_log_sender.clone(),
                        cancellation_token.clone(),
                        runtime_handle.clone(),
                    ) {
                        error!("{log_type} dedicated execution thread {i} exited with error: {e}");
                    }
                });
                thread_handles.push(handle);
            }
        }
        let message_sender = thread_pools.message_sender();
        (Self { message_sender }, ExecutorThreads { thread_handles })
    }

    /// Execute a message coming from a webhook, by sending it to the appropriate thread pool.
    /// That will be the thread pool dedicated to the message's type, if one exists, or the
    /// default thread pool for general execution.
    pub fn execute_webhook_message(
        self: &Self,
        message: Message,
    ) -> Result<(), TrySendError<Message>> {
        self.message_sender.try_send(message)
    }
}
