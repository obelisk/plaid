mod errors;
mod limits;
mod utils;

use errors::Errors;
use limits::LimitingTunables;
use lru::LruCache;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::{self};
use std::num::NonZeroUsize;
use std::sync::{Arc, RwLock};
use utils::{
    cost_function, get_module_computation_limit, get_module_page_count, read_and_configure_secrets,
    read_and_parse_modules,
};
use wasmer::{
    sys::BaseTunables, CompilerConfig, Cranelift, Engine, Module, NativeEngineExt, Pages, Target,
};
use wasmer_middlewares::Metering;

#[derive(Deserialize)]
pub struct LimitAmount {
    default: u64,
    log_type: HashMap<String, u64>,
    module_overrides: HashMap<String, u64>,
}

#[derive(Deserialize)]
pub struct Configuration {
    /// Where to load modules from
    pub module_dir: String,
    /// A list of rules that **cannot** be executed in parallel. We assume that rules can be executed
    /// in parallel unless otherwise noted. Rules that cannot execute in parallel wait until the
    /// executor is finished processing a rule before beginning their own execution.
    pub single_threaded_rules: Option<Vec<String>>,
    /// What the log type of a module should be if it's not the first part of the filename
    pub log_type_overrides: HashMap<String, String>,
    /// How much computation a module is allowed to do
    pub computation_amount: LimitAmount,
    /// How much memory a module is allowed to use
    pub memory_page_count: LimitAmount,
    /// The size of the LRU cache for each module. Not setting it means LRUs are disabled
    pub lru_cache_size: Option<usize>,
    /// The secrets that are available to modules. No actual secrets should be included in this map.
    /// Instead, the values here should be names of secrets whose values are present in
    /// the secrets file. This makes it possible for to check in your Plaid config without exposing secrets.
    pub secrets: HashMap<String, HashMap<String, String>>,
    /// See persistent_response_size in PlaidModule for an explanation on how to use this
    pub persistent_response_size: HashMap<String, usize>,
}

/// The persistent response allowed for the module. This is used for
/// modules to store data that was generated from their last invocation which can be
/// accessed by the next invocation or by GET requests configured to use it as a
/// response. The max size here only determines how much data can be stored, it does
/// not affect how much data can be returned by GET requests configured to use a module
/// as a data generator for a response.
pub struct PersistentResponse {
    pub max_size: usize,
    pub data: Arc<RwLock<Option<String>>>,
}

impl PersistentResponse {
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            data: Arc::new(RwLock::new(None)),
        }
    }

    pub fn get_data(&self) -> Result<Option<String>, ()> {
        match self.data.read() {
            Ok(data) => Ok(data.clone()),
            Err(e) => {
                error!(
                    "Critical error getting a read lock on persistent response: {:?}",
                    e
                );
                Err(())
            }
        }
    }
}

/// Defines a loaded Plaid module that can be run on incoming messages or to handle
/// GET requests.
pub struct PlaidModule {
    /// The name of the module
    pub name: String,
    /// The compiled WASM module
    pub module: Module,
    /// The WASM enginer used to run the module
    pub engine: Engine,
    /// The maximum computation allowed for the module
    pub computation_limit: u64,
    /// The maximum number of memory pages allowed to be mapped for the module
    pub page_limit: u32,
    /// Any defined secrets the module is allowed to access
    pub secrets: Option<HashMap<String, Vec<u8>>>,
    /// An LRU cache for the module if the runtime has allowed LRU caches for modules
    pub cache: Option<Arc<RwLock<LruCache<String, String>>>>,
    /// See the PersistentResponse type.
    pub persistent_response: Option<PersistentResponse>,
    pub parallel_execution_enabled: bool,
}

impl PlaidModule {
    pub fn get_persistent_response_data(&self) -> Option<String> {
        self.persistent_response
            .as_ref()
            .map(|x| x.get_data().ok().flatten())
            .flatten()
    }

    /// Configure and compiles a Plaid module with specified computation limits and memory page count.
    ///
    /// This function sets up the computation metering, configures the module tunables, and
    /// compiles the module using the provided bytecode and settings.
    ///
    /// This function returns a PlaidModule with `secrets`, `cache`, and `persistent_response` set to `None.`
    /// __Ensure that you set these values if needed after calling this function__.
    fn configure_and_compile(
        filename: &str,
        computation_amount: &LimitAmount,
        memory_page_count: &LimitAmount,
        module_bytes: Vec<u8>,
        log_type: &str,
        parallel_execution_enabled: bool,
    ) -> Result<Self, Errors> {
        // Get the computation limit for the module
        let computation_limit =
            get_module_computation_limit(computation_amount, &filename, log_type);

        // Get the memory limit for the module
        let page_limit = get_module_page_count(memory_page_count, &filename, log_type);

        let metering = Arc::new(Metering::new(computation_limit, cost_function));
        let mut compiler = Cranelift::default();
        compiler.push_middleware(metering);

        // Configure module tunables - this includes our computation limit and page count
        let base = BaseTunables::for_target(&Target::default());
        let tunables = LimitingTunables::new(base, Pages(page_limit));
        let mut engine: Engine = compiler.into();
        engine.set_tunables(tunables);

        // Compile the module using the middleware and tunables we just set up
        let mut module = Module::new(&engine, module_bytes).map_err(|e| {
            error!("Failed to compile module [{filename}]. Error: {e}");
            Errors::ModuleCompilationFailure
        })?;
        module.set_name(&filename);

        info!("Name: [{filename}] Computation Limit: [{computation_limit}] Memory Limit: [{page_limit} pages] Log Type: [{log_type}]");
        for import in module.imports() {
            info!("\tImport: {}", import.name());
        }

        Ok(Self {
            name: filename.to_string(),
            module,
            engine,
            computation_limit,
            page_limit,
            secrets: None,
            cache: None,
            persistent_response: None,
            parallel_execution_enabled,
        })
    }
}

/// We need multiple ways of referencing the modules. To prevent duplication we use `Arc`s.
/// Since modules are static and are executed in ephemeral instances this should be fine.
#[derive(Default)]
pub struct PlaidModules {
    channels: HashMap<String, Vec<Arc<PlaidModule>>>,
    modules: HashMap<String, Arc<PlaidModule>>,
}

impl PlaidModules {
    /// All of the modules are `Arc`s so this should be relatively inexpensive. Generally
    /// this is used for the executor to handle logs coming in and processing them through
    /// the correct channel of modules.
    pub fn get_channels(&self) -> HashMap<String, Vec<Arc<PlaidModule>>> {
        self.channels.clone()
    }

    /// All of the modules are `Arc`s so this should be relatively inexpensive. Generally this
    /// is used for the GET request system so that we can reference which module is to serve a
    /// particular webhook's GET handle.
    pub fn get_modules(&self) -> HashMap<String, Arc<PlaidModule>> {
        self.modules.clone()
    }

    /// Get a particular module by name. This makes the API ergonomic enough
    /// we don't need to exposure the underlying data structures.
    pub fn get_module(&self, name: &str) -> Option<Arc<PlaidModule>> {
        self.modules.get(name).cloned()
    }
}

pub fn load(config: Configuration) -> Result<PlaidModules, ()> {
    let module_paths = fs::read_dir(config.module_dir).unwrap();

    let mut modules = PlaidModules::default();

    let byte_secrets = read_and_configure_secrets(&config.secrets);

    for path in module_paths {
        let (filename, module_bytes) = match path {
            Ok(path) => {
                if let Ok(filename_and_bytes) = read_and_parse_modules(&path) {
                    filename_and_bytes
                } else {
                    continue;
                }
            }
            Err(e) => {
                error!("Bad entry in modules directory - skipping. Error: {e}");
                continue;
            }
        };

        // See if a type is defined in the configuration file, if not then we will grab the first part
        // of the filename up to the first underscore.
        let type_ = if let Some(type_) = config.log_type_overrides.get(&filename) {
            type_.to_string()
        } else {
            let type_: Vec<&str> = filename.split('_').collect();
            type_[0].to_string()
        };

        // Check if this rule can be executed in parallel
        let parallel_execution_enabled = config
            .single_threaded_rules
            .as_ref()
            .map_or(true, |rules| rules.contains(&filename));

        // Configure and compile module
        let Ok(mut plaid_module) = PlaidModule::configure_and_compile(
            &filename,
            &config.computation_amount,
            &config.memory_page_count,
            module_bytes,
            &type_,
            parallel_execution_enabled,
        ) else {
            continue;
        };

        // Configure cache for module
        let cache = config.lru_cache_size.and_then(|size| {
            if size == 0 {
                None // No cache if provided size is 0
            } else {
                NonZeroUsize::new(size)
                    .map(|non_zero_size| Arc::new(RwLock::new(LruCache::new(non_zero_size))))
            }
        });

        // Persistent response is available to be set per module. This allows it to persistently
        // store the result of its run. It can use this during further runs, or it can be used
        // as the target of GET request hooks.
        let persistent_response = config
            .persistent_response_size
            .get(&filename)
            .copied()
            .map(PersistentResponse::new);

        // Set optional fields on our new module
        plaid_module.cache = cache;
        plaid_module.persistent_response = persistent_response;
        plaid_module.secrets = byte_secrets.get(&type_).map(|x| x.clone());

        // Put it in an Arc because we're going to have multiple references to it
        let plaid_module = Arc::new(plaid_module);

        // Insert into the channels map
        if let Some(mods) = modules.channels.get_mut(&type_) {
            mods.push(plaid_module.clone());
        } else {
            modules.channels.insert(type_, vec![plaid_module.clone()]);
        }

        // Insert into the name map
        modules.modules.insert(filename, plaid_module);
    }

    Ok(modules)
}
