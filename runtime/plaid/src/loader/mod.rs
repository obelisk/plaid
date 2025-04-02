mod errors;
mod limits;
mod signing;
mod utils;

use errors::Errors;
use limits::LimitingTunables;
use lru::LruCache;
use serde::{de, Deserialize, Serialize};
use signing::check_module_signatures;
use sshcerts::PublicKey;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::{self};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, RwLock};
use utils::{
    cost_function, get_module_computation_limit, get_module_page_count,
    get_module_persistent_storage_limit, read_and_configure_secrets, read_and_parse_modules,
};
use wasmer::{
    sys::BaseTunables, CompilerConfig, Cranelift, Engine, Module, NativeEngineExt, Pages, Target,
};
use wasmer_middlewares::Metering;

use crate::storage::Storage;

/// Limit imposed on some resource
#[derive(Deserialize)]
pub struct LimitedAmount {
    /// The limit's default value
    default: u64,
    /// Override values based on log type
    log_type: HashMap<String, u64>,
    /// Override values based on module names
    module_overrides: HashMap<String, u64>,
}

/// Represents the value of a limit imposed on some resource.
/// This can be a finite value (u64, with 0 a valid value) or
/// it can be unlimited. These are the TOML encodings for the
/// two cases:
/// * "Unlimited"
/// * { Limited = value }
///
/// E.g.,
/// ```
/// [loading.storage_size]
/// default = "Unlimited"
/// [loading.storage_size.log_type]
/// [loading.storage_size.module_overrides]
/// "test_db.wasm" = { Limited = 50 }
/// ```
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LimitValue {
    Unlimited,
    Limited(u64),
}

impl Display for LimitValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LimitValue::Unlimited => write!(f, "Unlimited"),
            LimitValue::Limited(v) => {
                let disp = format!("Limited({v})");
                f.write_str(&disp)
            }
        }
    }
}

/// Limit imposed on some resource which also supports Unlimited
#[derive(Deserialize)]
pub struct LimitableAmount {
    /// The limit's default value
    default: LimitValue,
    /// Override values based on log type
    log_type: HashMap<String, LimitValue>,
    /// Override values based on module names
    module_overrides: HashMap<String, LimitValue>,
}

/// Configuration for loading Plaid modules
#[derive(Deserialize)]
pub struct Configuration {
    /// Where to load modules from
    pub module_dir: String,
    /// A list of case-insensitive rule names that **cannot** be executed in parallel. We assume that rules can be executed
    /// in parallel unless otherwise noted. Rules that cannot execute in parallel wait until the
    /// executor is finished processing a rule before beginning their own execution.
    pub single_threaded_rules: Option<Vec<String>>,
    /// What the log type of a module should be if it's not the first part of the filename
    pub log_type_overrides: HashMap<String, String>,
    /// How much computation a module is allowed to do
    pub computation_amount: LimitedAmount,
    /// How much memory a module is allowed to use
    pub memory_page_count: LimitedAmount,
    /// How many bytes a module is allowed to store in persistent storage
    pub storage_size: LimitableAmount,
    /// The size of the LRU cache for each module. Not setting it means LRUs are disabled
    pub lru_cache_size: Option<usize>,
    /// The secrets that are available to modules. No actual secrets should be included in this map.
    /// Instead, the values here should be names of secrets whose values are present in
    /// the secrets file. This makes it possible for to check in your Plaid config without exposing secrets.
    /// The mapping is `{log_type -> {secret_name -> secret_value}}`.
    pub secrets: HashMap<String, HashMap<String, String>>,
    /// Accessory data which is available to all rules (unless overridden by the dedicated override config).
    /// The mapping is `{key -> value}``
    pub universal_accessory_data: Option<HashMap<String, String>>,
    /// Per-log-type accessory data that is added to universal accessory data for the given log type. In case
    /// of a name clash, this takes precedence.
    /// The mapping is `{log_type -> {key -> value}}`
    pub accessory_data_log_type_overrides: HashMap<String, HashMap<String, String>>,
    /// Per-rule accessory data that is added to universal accessory data and per-log-type accessory data. In
    /// case of a name clash, this takes precedence over everything else.
    /// The mapping is `{rule_file_name -> {key -> value}}`
    pub accessory_data_file_overrides: HashMap<String, HashMap<String, String>>,
    /// See persistent_response_size in PlaidModule for an explanation on how to use this
    pub persistent_response_size: HashMap<String, usize>,
    /// Modules will be loaded in test_mode meaning they will not be able to make any API calls that
    /// cause side effects. This does not include:
    /// * Storage
    /// * Cache
    /// * Persistent Response
    /// * Some MNRs: Each MNR must decorate themselves as being available in test mode for them to be available.
    /// What an API does in this mode is up to the API implementation and the relevant API module
    /// should be consulted.
    #[serde(default)]
    pub test_mode: bool,
    /// List of modules that should be exempt from being tested. This will allow all APIs to be called,
    /// even if they have side effects.
    #[serde(default)]
    pub test_mode_exemptions: Vec<String>,
    /// Configuration for module signing. If defined, we require that ALL
    /// module are signed by a set of authorized signers
    pub module_signing: Option<ModuleSigningConfiguration>,
}

/// This structure defines the parameters required to validate signatures for modules.
#[derive(Deserialize)]
pub struct ModuleSigningConfiguration {
    /// A list of authorized signer key fingerprints.
    ///
    /// This list should contain the fingerprints of the keys belonging to the authorized signers,
    /// typically the administrators responsible for managing the Plaid instance.
    #[serde(deserialize_with = "pubkey_deserializer")]
    pub authorized_signers: Vec<PublicKey>,
    /// Where to load signatures from. Defaults to `../module_signatures` if no
    /// value is provided
    #[serde(default = "default_sig_dir")]
    pub signatures_dir: String,
    /// The namespace of the signature
    pub signature_namespace: String,
    /// The number of valid signatures required on each module
    pub signatures_required: usize,
}

/// Deserializer for a public key
fn pubkey_deserializer<'de, D>(deserializer: D) -> Result<Vec<PublicKey>, D::Error>
where
    D: de::Deserializer<'de>,
{
    let raw = Vec::<String>::deserialize(deserializer)?;
    Ok(raw
        .iter()
        .filter_map(|key| {
            PublicKey::from_string(key)
                .map_err(|e| {
                    error!("Invalid public key provided: {key} - skipping. Error: {e}");
                    e
                })
                .ok()
        })
        .collect())
}

/// The default directory to look for module signatures in if none is provided
fn default_sig_dir() -> String {
    "../module_signatures".to_string()
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
    /// The number of bytes the module is currently saving in persistent storage
    pub storage_current: Arc<RwLock<u64>>,
    /// The maximum number of bytes the module can save in persistent storage
    pub storage_limit: LimitValue,
    /// Any additional data the module is given at loading time
    pub accessory_data: Option<HashMap<String, Vec<u8>>>,
    /// Any defined secrets the module is allowed to access
    pub secrets: Option<HashMap<String, Vec<u8>>>,
    /// An LRU cache for the module if the runtime has allowed LRU caches for modules
    pub cache: Option<Arc<RwLock<LruCache<String, String>>>>,
    /// See the PersistentResponse type.
    pub persistent_response: Option<PersistentResponse>,
    /// Indicates whether the module is safe for concurrent execution.
    ///
    /// - If `None`, the module can be executed concurrently without any restrictions.
    /// - If `Some`, the module is marked as unsafe for concurrent execution, and the `Mutex<()>`
    ///   is used to ensure mutual exclusion, preventing multiple threads from executing it simultaneously.
    pub concurrency_unsafe: Option<Mutex<()>>,
    /// If the module is in test mode, meaning it should not be allowed to cause side effects
    pub test_mode: bool,
}

impl std::fmt::Display for PlaidModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
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
    async fn configure_and_compile(
        filename: &str,
        computation_amount: &LimitedAmount,
        memory_page_count: &LimitedAmount,
        storage_amount: &LimitableAmount,
        storage: Option<Arc<Storage>>,
        module_bytes: Vec<u8>,
        log_type: &str,
        concurrency_unsafe: Option<Mutex<()>>,
        test_mode: bool,
    ) -> Result<Self, Errors> {
        // Get the computation limit for the module
        let computation_limit =
            get_module_computation_limit(computation_amount, &filename, log_type);

        // Get the memory limit for the module
        let page_limit = get_module_page_count(memory_page_count, &filename, log_type);

        // Get the persistent storage limit
        let storage_limit =
            get_module_persistent_storage_limit(storage_amount, &filename, log_type);

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

        // Count bytes already in storage
        let storage_current_bytes: u64 = match storage {
            None => 0,
            Some(s) => s.get_namespace_byte_size(filename).await.unwrap(),
        };
        let storage_current = Arc::new(RwLock::new(storage_current_bytes));

        info!("Name: [{filename}] Computation Limit: [{computation_limit}] Memory Limit: [{page_limit} pages] Storage: [{storage_current_bytes}/{storage_limit} bytes used] Log Type: [{log_type}]. Concurrency Safe: [{}] Test Mode: [{test_mode}]", concurrency_unsafe.is_none());
        for import in module.imports() {
            info!("\tImport: {}", import.name());
        }

        Ok(Self {
            name: filename.to_string(),
            module,
            engine,
            computation_limit,
            storage_current,
            storage_limit,
            page_limit,
            accessory_data: None,
            secrets: None,
            cache: None,
            persistent_response: None,
            concurrency_unsafe,
            test_mode,
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

/// Load all modules, according to Plaid's configuration
pub async fn load(
    config: Configuration,
    storage: Option<Arc<Storage>>,
) -> Result<PlaidModules, ()> {
    let module_paths = fs::read_dir(config.module_dir.clone()).unwrap();

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

        // Fetch and verify the corresponding signature over this module if we require
        // rule signing. If any rule does not have enough valid signatures it will not be loaded.
        if let Some(signing) = &config.module_signing {
            if check_module_signatures(signing, &filename, &module_bytes).is_err() {
                continue;
            }
        }

        // See if a type is defined in the configuration file, if not then we will grab the first part
        // of the filename up to the first underscore.
        let type_ = if let Some(type_) = config.log_type_overrides.get(&filename) {
            type_.to_string()
        } else {
            let type_: Vec<&str> = filename.split('_').collect();
            type_[0].to_string()
        };

        // Check if this rule can be executed in parallel
        let concurrency_safe = config.single_threaded_rules.as_ref().map_or(None, |rules| {
            if rules
                .iter()
                .any(|rule| rule.eq_ignore_ascii_case(&filename))
            {
                Some(Mutex::new(()))
            } else {
                None
            }
        });

        // Default is the global test mode. Then if the module is in the exemptions specification
        // we will disable test mode for that module.
        let test_mode = config.test_mode && !config.test_mode_exemptions.contains(&filename);

        // Configure and compile module
        let Ok(mut plaid_module) = PlaidModule::configure_and_compile(
            &filename,
            &config.computation_amount,
            &config.memory_page_count,
            &config.storage_size,
            storage.clone(),
            module_bytes,
            &type_,
            concurrency_safe,
            test_mode,
        )
        .await
        else {
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
        plaid_module.accessory_data = module_accessory_data(&config, &plaid_module.name, &type_);

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

/// Read the loading configuration and some data about the module, and return the optional
/// accessory data that the module will have access to.
fn module_accessory_data(
    config: &Configuration,
    filename: &str,
    logtype: &str,
) -> Option<HashMap<String, Vec<u8>>> {
    // If we have some universal accessory data, then we set it...
    let mut accessory_data: Option<HashMap<String, Vec<u8>>> = match config.universal_accessory_data
    {
        Some(ref uad) => Some(
            uad.iter()
                .map(|v| (v.0.to_string(), v.1.as_bytes().to_vec()))
                .collect(),
        ),
        None => None,
    };

    // ... then we add entries which are specified in the per-log-type accessory data, overwriting those with the same name.
    if let Some(logtype_overrides) = config.accessory_data_log_type_overrides.get(logtype) {
        // If we already had accessory data, start from there. Otherwise, start from an empty map
        let mut tmp_accessory_data = accessory_data.unwrap_or(HashMap::new());
        for (key, value) in logtype_overrides {
            tmp_accessory_data.insert(key.to_string(), value.as_bytes().to_vec());
        }
        accessory_data = Some(tmp_accessory_data);
    }

    // ... then we add entries which are specified in the per-rule accessory data, overwriting those with the same name.
    if let Some(file_overrides) = config.accessory_data_file_overrides.get(filename) {
        // If we already had accessory data, start from there. Otherwise, start from an empty map
        let mut tmp_accessory_data = accessory_data.unwrap_or(HashMap::new());
        for (key, value) in file_overrides {
            tmp_accessory_data.insert(key.to_string(), value.as_bytes().to_vec());
        }
        accessory_data = Some(tmp_accessory_data);
    }

    accessory_data
}
