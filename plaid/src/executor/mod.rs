use crate::apis::Api;

use crate::functions::{link_functions_to_module, LinkError};
use crate::loader::PlaidModule;
use crate::logging::{Logger, LoggingError};
use crate::storage::Storage;

use crossbeam_channel::Receiver;

use plaid_stl::messages::{LogSource, LogbacksAllowed};
use serde::{Deserialize, Serialize};
use wasmer::{FunctionEnv, Imports, Instance, Memory, RuntimeError, Store, TypedFunction};
use wasmer_middlewares::metering::{get_remaining_points, MeteringPoints};

use lru::LruCache;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub type_: String,
    pub data: Vec<u8>,
    pub accessory_data: HashMap<String, Vec<u8>>,
    pub source: LogSource,
    pub logbacks_allowed: LogbacksAllowed,
}

impl Message {
    pub fn new(
        type_: String,
        data: Vec<u8>,
        source: LogSource,
        logbacks_allowed: LogbacksAllowed,
    ) -> Self {
        Self {
            type_,
            data,
            accessory_data: HashMap::new(),
            source,
            logbacks_allowed,
        }
    }
}

#[derive(Clone)]
pub struct Env {
    // Name of the current module
    pub name: String,
    // Cache if available
    pub cache: Option<Arc<RwLock<LruCache<String, String>>>>,
    // The message that is being processed.
    pub message: Message,
    // A handle to the API to make external calls
    pub api: Arc<Api>,
    // A handle to the storage system if one is configured
    pub storage: Option<Arc<Storage>>,
    // A sender to the external logging system
    pub external_logging_system: Logger,
    pub memory: Option<Memory>,
    // A special value that can be filled to leave a string response available after
    // the module has execute. Generally this is used for GET mode responses.
    pub response: Option<String>,
}

pub struct Executor {
    _handles: Vec<JoinHandle<Result<(), ExecutorError>>>,
    api: Arc<Api>,
    storage: Option<Arc<Storage>>,
    els: Logger,
}

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

pub enum ModuleExecutionError {
    ComputationExhausted(u64),
    ModuleErrorCode(i32),
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
                write!(f, "Computation Exhaused. Limit: [{limit}]")
            }
            ModuleExecutionError::ModuleErrorCode(error_code) => {
                write!(f, "Module returned error code. Code: [{error_code}]")
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
    mut message: Message,
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

    // Load the secrets if they exist. This overwrites any thing that is already
    // in the accessory data meaning secrets always take precedence over possible user
    // provided data, like headers (the other major use case for accessory data)
    if let Some(secrets) = &plaid_module.secrets {
        message.accessory_data.extend(secrets.clone());
    }

    let env = Env {
        name: plaid_module.name.clone(),
        cache: plaid_module.cache.clone(),
        message: message.clone(),
        api: api.clone(),
        storage: storage.clone(),
        external_logging_system: els.clone(),
        memory: None,
        response,
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

fn update_persistent_response(
    plaid_module: &Arc<PlaidModule>,
    env: &FunctionEnv<Env>,
    mut store: &mut Store,
) -> Result<(), ExecutorError> {
    match (
        env.as_mut(&mut store).response.clone(),
        &plaid_module.persistent_response,
    ) {
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
                        *data = response;
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

fn execution_loop(
    receiver: Receiver<Message>,
    modules: HashMap<String, Vec<Arc<PlaidModule>>>,
    api: Arc<Api>,
    storage: Option<Arc<Storage>>,
    els: Logger,
) -> Result<(), ExecutorError> {
    // Wait on our receiver for logs to come in
    while let Ok(message) = receiver.recv() {
        // Check that we know what modules to send this new log to
        let execution_modules = match modules.get(&message.type_) {
            None => {
                warn!(
                    "Got logs of a type we have no modules for? Type was: {}",
                    message.type_
                );
                continue;
            }
            Some(module) => module,
        };

        // For every module that operates on that log type
        for plaid_module in execution_modules {
            // TODO @obelisk: This will quietly swallow locking errors on the persistent response
            // This will eventually be caught if something tries to update the response but I don't
            // know if that's good enough.
            let persistent_response = plaid_module.get_persistent_response_data();
            // Message needs to be cloned because of the logback budget
            // which is separate for every rule running the same message.
            let (mut store, instance, entrypoint, env) = match prepare_for_execution(
                message.clone(),
                plaid_module.clone(),
                api.clone(),
                storage.clone(),
                els.clone(),
                persistent_response,
            ) {
                Ok((store, instance, ep, env)) => (store, instance, ep, env),
                Err(e) => {
                    els.log_module_error(
                        plaid_module.name.clone(),
                        format!("Failed to prepare for execution: {e}"),
                        message.data.clone(),
                    )?;
                    continue;
                }
            };

            let computation_limit = plaid_module.computation_limit;
            // Call the entrypoint
            let error = match entrypoint.call(&mut store) {
                Ok(n) => {
                    if n != 0 {
                        Some(ModuleExecutionError::ModuleErrorCode(n))
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
                                format!("{}_computation_percentage_used", plaid_module.name),
                                computation_used as i64,
                            )?;
                        }
                        None
                    }
                }
                Err(e) => Some(determine_error(e, computation_limit, &instance, &mut store)),
            };

            // If there was an error then log that it happened to the els
            if let Some(error) = error {
                els.log_module_error(
                    plaid_module.name.clone(),
                    format!("{error}"),
                    message.data.clone(),
                )?;

                // Stop processing this log and move on to the next one
                continue;
            }

            // Update the persistent response
            update_persistent_response(&plaid_module, &env, &mut store)?;
        }
    }
    Err(ExecutorError::IncomingLogError)
}

fn determine_error(
    e: RuntimeError,
    computation_limit: u64,
    instance: &Instance,
    mut store: &mut Store,
) -> ModuleExecutionError {
    // First check to see if we've exhausted computation
    if let MeteringPoints::Exhausted = get_remaining_points(&mut store, &instance) {
        return ModuleExecutionError::ComputationExhausted(computation_limit);
    }

    // If all else fails, it's an unknown error
    ModuleExecutionError::UnknownExecutionError(format!("{e}"))
}

impl Executor {
    pub fn new(
        receiver: Receiver<Message>,
        modules: HashMap<String, Vec<Arc<PlaidModule>>>,
        api: Arc<Api>,
        storage: Option<Arc<Storage>>,
        execution_threads: u8,
        els: Logger,
    ) -> Self {
        let mut _handles = vec![];
        for i in 0..execution_threads {
            info!("Starting Execution Thread {i}");
            let receiver = receiver.clone();
            let api = api.clone();
            let storage = storage.clone();
            let modules = modules.clone();
            let els = els.clone();
            _handles.push(thread::spawn(move || {
                execution_loop(receiver, modules, api, storage, els)
            }));
        }

        Self {
            _handles,
            api,
            storage,
            els,
        }
    }

    /// For executing a module immediately and getting the response back from it
    pub fn immediate_execute(
        &self,
        message: Message,
        plaid_module: Arc<PlaidModule>,
    ) -> Result<Option<String>, ExecutorError> {
        let computation_limit = plaid_module.computation_limit;
        let name = plaid_module.name.clone();

        let persistent_response = plaid_module
            .persistent_response
            .as_ref()
            .map(|pr| pr.data.read().and_then(|x| Ok(x.to_string())).ok())
            .flatten();

        let (mut store, instance, entrypoint, env) = prepare_for_execution(
            message,
            plaid_module.clone(),
            self.api.clone(),
            self.storage.clone(),
            self.els.clone(),
            persistent_response,
        )?;

        match entrypoint.call(&mut store) {
            Ok(n) => {
                if n != 0 {
                    Err(ExecutorError::ModuleExecutionError(
                        ModuleExecutionError::ModuleErrorCode(n),
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
                        self.els.log_ts(
                            format!("{}_immediate_computation_percentage_used", name),
                            computation_used as i64,
                        )?;
                    }
                    update_persistent_response(&plaid_module, &env, &mut store)?;

                    // Return the optional response
                    Ok(env.as_mut(&mut store).response.clone())
                }
            }
            Err(e) => Err(ExecutorError::ModuleExecutionError(determine_error(
                e,
                computation_limit,
                &instance,
                &mut store,
            ))),
        }
    }
}
