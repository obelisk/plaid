use crate::apis::Api;

use crate::functions::{
    create_bindgen_externref_xform, create_bindgen_placeholder, link_functions_to_module, LinkError,
};
use crate::loader::PlaidModule;
use crate::logging::{Logger, LoggingError};
use crate::performance::ModulePerformanceMetadata;
use crate::storage::Storage;

use crossbeam_channel::{Receiver, Sender};
use tokio::sync::oneshot::Sender as OneShotSender;

use plaid_stl::messages::{LogSource, LogbacksAllowed};
use serde::{Deserialize, Serialize};
use wasmer::{FunctionEnv, Imports, Instance, Memory, RuntimeError, Store, TypedFunction};
use wasmer_middlewares::metering::{get_remaining_points, MeteringPoints};

use std::collections::HashMap;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// When a rule is used to generate a response to a GET request, this structure
/// is what is passed from the executor to the async webhook runtime.
#[derive(Serialize, Deserialize)]
pub struct ResponseMessage {
    /// Currently unused because there is no API to set this and how it should
    /// treated by the higher level cache is still to be defined.
    pub code: u16,
    /// The data the rule intends to return in the serviced GET request.
    pub body: String,
}

/// A message to be processed by one or more modules
#[derive(Serialize, Deserialize)]
pub struct Message {
    /// A unique identifier for this message
    pub id: String,
    /// The message channel that this message is going to run on
    pub type_: String,
    /// The data passed to the module
    pub data: Vec<u8>,
    /// Any headers the module will have access to, while processing this message
    pub headers: HashMap<String, Vec<u8>>,
    /// Any query parameters the module will have access to, while processing this message
    pub query_params: HashMap<String, Vec<u8>>,
    /// Where the message came from
    pub source: LogSource,
    /// If this message is allowed to trigger additional messages to the same
    /// or other message channels
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
        headers: HashMap<String, Vec<u8>>,
        query_params: HashMap<String, Vec<u8>>,
        response_sender: Option<OneShotSender<Option<ResponseMessage>>>,
        module: Option<Arc<PlaidModule>>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            type_,
            data,
            headers,
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
    // A sender to the external logging system
    pub external_logging_system: Logger,
    /// Memory for host-guest communication
    pub memory: Option<Memory>,
    // A special value that can be filled to leave a string response available after
    // the module has execute. Generally this is used for GET mode responses.
    pub response: Option<String>,
    // Context about error encountered by the module during its execution
    pub execution_error_context: Option<String>,
}

/// The executor that processes messages
pub struct Executor {
    _handles: Vec<JoinHandle<Result<(), ExecutorError>>>,
}

/// Errors encountered by the executor while trying to execute a module
pub enum ExecutorError {
    ExternalLoggingError(LoggingError),
    IncomingLogError,
    LinkError(LinkError),
    InstantiationError(String),
    MemoryError(String),
    NoEntrypoint,
    InvalidEntrypoint,
    ModuleExecutionError(ModuleExecutionError),
}

impl std::fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutorError::ExternalLoggingError(e) => write!(f, "External Logging Error: {e}"),
            ExecutorError::IncomingLogError => write!(f, "Incoming Log Error"),
            ExecutorError::LinkError(e) => write!(f, "Link Error: {e}"),
            ExecutorError::InstantiationError(e) => write!(f, "Instantiation Error: {e}"),
            ExecutorError::MemoryError(e) => write!(f, "Memory Error: {e}"),
            ExecutorError::NoEntrypoint => write!(f, "No entrypoint found in module"),
            ExecutorError::InvalidEntrypoint => write!(
                f,
                "Entrypoint is not a function or not the correct prototype"
            ),
            ExecutorError::ModuleExecutionError(e) => write!(f, "Module Execution Error: {e}"),
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

/// Take a message, a module, and an executor and get an instance back that is ready to run
/// the provided module.
fn prepare_for_execution(
    message: Message,
    plaid_module: Arc<PlaidModule>,
    api: Arc<Api>,
    storage: Option<Arc<Storage>>,
    els: Logger,
    response: Option<String>,
) -> Result<(Store, Instance, TypedFunction<(), i32>, FunctionEnv<Env>), ExecutorError> {
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
        external_logging_system: els.clone(),
        memory: None,
        response,
        execution_error_context: None,
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
        create_bindgen_placeholder(&mut store),
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

    Ok((store, instance, ep, envr))
}

/// Update a module's persistent response
fn update_persistent_response(
    plaid_module: &Arc<PlaidModule>,
    env: &FunctionEnv<Env>,
    mut store: &mut Store,
    response_sender: Option<OneShotSender<Option<ResponseMessage>>>,
) -> Result<(), ExecutorError> {
    match (
        env.as_mut(&mut store).response.clone(),
        &plaid_module.persistent_response,
    ) {
        (None, _) => {
            // We need to check if there might be a tokio task serving a GET
            // that is waiting on this response. If the rule doesn't give one, we
            // need to ensure we send a None to wake up that task and complete it
            if let Some(sender) = response_sender {
                if let Err(_) = sender.send(None) {
                    error!("[{}] was servicing a request that returned no response and failed to send!", plaid_module.name);
                } else {
                    error!(
                        "[{}] was servicing a request that returned no response!",
                        plaid_module.name
                    );
                }
            }
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
                        if let Some(sender) = response_sender {
                            if let Err(_) = sender.send(Some(ResponseMessage {
                                code: 200,
                                body: response,
                            })) {
                                error!(
                                    "[{}] was servicing a request but sending the response failed!",
                                    plaid_module.name
                                );
                            }
                        }
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
fn process_message_with_module(
    message: Message,
    module: Arc<PlaidModule>,
    api: Arc<Api>,
    storage: Option<Arc<Storage>>,
    els: Logger,
    performance_mode: Option<Sender<ModulePerformanceMetadata>>,
) -> Result<(), ExecutorError> {
    // For every module that operates on that log type
    // Mark this rule as currently being processed by locking the mutex
    // This lock will be dropped at the end of the iteration so we don't
    // need to handle unlocking it
    let _lock = match module.concurrency_unsafe {
        Some(ref mutex) => match mutex.lock() {
            Ok(guard) => Some(guard),
            Err(p_err) => {
                error!(
                    "Lock was poisoned on [{}]. Clearing and continuing: {p_err}.",
                    module.name
                );
                mutex.clear_poison();
                mutex.lock().ok()
            }
        },
        None => None,
    };
    // TODO @obelisk: This will quietly swallow locking errors on the persistent response
    // This will eventually be caught if something tries to update the response but I don't
    // know if that's good enough.
    let persistent_response = module.get_persistent_response_data();
    // Message needs to be cloned because of the logback budget
    // which is separate for every rule running the same message.
    let (mut store, instance, entrypoint, env) = match prepare_for_execution(
        message.create_duplicate(),
        module.clone(),
        api.clone(),
        storage.clone(),
        els.clone(),
        persistent_response,
    ) {
        Ok((store, instance, ep, env)) => (store, instance, ep, env),
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
    // Call the entrypoint
    let begin = Instant::now();
    let error = match entrypoint.call(&mut store) {
        Ok(n) => {
            if n != 0 {
                Some(ModuleExecutionError::ModuleError(
                    env.as_ref(&store)
                        .execution_error_context
                        .clone()
                        .unwrap_or("None".to_string()),
                ))
            } else {
                // This should always work because when computation is exhausted,
                // we end up in the RuntimeError block.
                if let MeteringPoints::Remaining(remaining) =
                    get_remaining_points(&mut store, &instance)
                {
                    let computation_remaining_percentage =
                        (remaining as f32 / computation_limit as f32) * 100.;
                    let computation_used = 100. - computation_remaining_percentage;
                    els.log_ts(
                        format!("{}_computation_percentage_used", module.name),
                        computation_used as i64,
                    )?;
                    els.log_ts(
                        format!("{}_execution_duration", module.name),
                        begin.elapsed().as_micros() as i64,
                    )?;

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
                }

                None
            }
        }
        Err(e) => Some(determine_error(
            e,
            computation_limit,
            &instance,
            &mut store,
            &env,
        )),
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

    // Update the persistent response
    if let Err(e) = update_persistent_response(&module, &env, &mut store, message.response_sender) {
        els.log_module_error(
            module.name.clone(),
            format!("Failed to update persistent response: {e}"),
            message.data.clone(),
        )
        .unwrap();
    }

    Ok(())
}

fn execution_loop(
    receiver: Receiver<Message>,
    modules: HashMap<String, Vec<Arc<PlaidModule>>>,
    api: Arc<Api>,
    storage: Option<Arc<Storage>>,
    els: Logger,
    performance_monitoring_mode: Option<Sender<ModulePerformanceMetadata>>,
) -> Result<(), ExecutorError> {
    // Wait on our receiver for logs to come in
    while let Ok(message) = receiver.recv() {
        // Check that we know what modules to send this new log to
        match (&message.module, modules.get(&message.type_)) {
            // If this message has a response sender, we only
            // want to run it on that rule, not any defined logging
            // channel.
            (Some(ref module), _) => {
                let module = module.clone();
                process_message_with_module(
                    message,
                    module,
                    api.clone(),
                    storage.clone(),
                    els.clone(),
                    performance_monitoring_mode.clone(),
                )?;
            }
            (None, Some(modules)) => {
                // For every module that operates on that log type
                for module in modules {
                    process_message_with_module(
                        message.create_duplicate(),
                        module.clone(),
                        api.clone(),
                        storage.clone(),
                        els.clone(),
                        performance_monitoring_mode.clone(),
                    )?;
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
    Err(ExecutorError::IncomingLogError)
}

fn determine_error(
    e: RuntimeError,
    computation_limit: u64,
    instance: &Instance,
    mut store: &mut Store,
    env: &FunctionEnv<Env>,
) -> ModuleExecutionError {
    // First check to see if we've exhausted computation
    if let MeteringPoints::Exhausted = get_remaining_points(&mut store, &instance) {
        return ModuleExecutionError::ComputationExhausted(computation_limit);
    }

    // If all else fails, it's an unknown error
    ModuleExecutionError::UnknownExecutionError(format!(
        "{e}. Additional context: {}",
        env.as_ref(&store)
            .execution_error_context
            .clone()
            .unwrap_or("This is probably an OOM error".to_string())
    ))
}

impl Executor {
    pub fn new(
        receiver: Receiver<Message>,
        modules: HashMap<String, Vec<Arc<PlaidModule>>>,
        api: Arc<Api>,
        storage: Option<Arc<Storage>>,
        execution_threads: u8,
        els: Logger,
        performance_monitoring_mode: Option<Sender<ModulePerformanceMetadata>>,
    ) -> Self {
        let mut _handles = vec![];
        for i in 0..execution_threads {
            info!("Starting Execution Thread {i}");
            let receiver = receiver.clone();
            let api = api.clone();
            let storage = storage.clone();
            let modules = modules.clone();
            let els = els.clone();
            let performance_sender = performance_monitoring_mode.clone();
            _handles.push(thread::spawn(move || {
                execution_loop(receiver, modules, api, storage, els, performance_sender)
            }));
        }

        Self { _handles }
    }
}
