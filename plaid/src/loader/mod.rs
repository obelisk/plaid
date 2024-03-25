mod limits;

use limits::LimitingTunables;

use lru::LruCache;
use serde::Deserialize;
use wasmer::{wasmparser::Operator, CompilerConfig, Cranelift, Module};
use wasmer::{BaseTunables, Engine, NativeEngineExt, Pages, Target};

use wasmer_middlewares::Metering;

use std::collections::HashMap;
use std::fs;
use std::num::NonZeroUsize;
use std::sync::{Arc, RwLock};

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
    /// What the log type of a module should be if it's not the first part of the filename
    pub log_type_overrides: HashMap<String, String>,
    /// How much computation a module is allowed to do
    pub computation_amount: LimitAmount,
    /// How much memory a module is allowed to use
    pub memory_page_count: LimitAmount,
    /// The size of the LRU cache for each module. Not setting it means LRUs are disabled
    pub lru_cache_size: Option<usize>,
    /// The secrets that are available to modules
    pub secrets: HashMap<String, HashMap<String, String>>,
}

pub struct PlaidModule {
    pub name: String,
    pub module: Module,
    pub engine: Engine,
    pub computation_limit: u64,
    pub page_limit: u32,
    pub secrets: Option<HashMap<String, Vec<u8>>>,
    pub cache: Option<Arc<RwLock<LruCache<String, String>>>>,
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

const CALL_COST: u64 = 10;

pub fn load(
    config: Configuration,
) -> Result<PlaidModules, ()> {
    let module_paths = fs::read_dir(config.module_dir).unwrap();

    let mut modules = PlaidModules::default();

    let cost_function = |operator: &Operator| -> u64 {
        match operator {
            Operator::Call { .. } => CALL_COST,
            Operator::CallIndirect { .. } => CALL_COST,
            Operator::ReturnCall { .. } => CALL_COST,
            Operator::ReturnCallIndirect { .. } => CALL_COST,
            _ => 1,
        }
    };

    let byte_secrets: HashMap<String, HashMap<String, Vec<u8>>> = config
        .secrets
        .into_iter()
        .map(|(key, value)| {
            (
                key,
                value
                    .into_iter()
                    .map(|(inner_key, inner_value)| (inner_key, inner_value.as_bytes().to_vec()))
                    .collect(),
            )
        })
        .collect();

    for path in module_paths {
        // Get the module file name and read in the bytes
        let (filename, module_bytes) = if let Ok(path) = path {
            // Path's can be weird so we just try to make it a UTF8 string,
            // if it's not UTF8, we'll fail reading it and skip it.
            let filename = path.file_name().to_string_lossy().to_string();

            // Also skip any files that aren't wasm files
            if !filename.ends_with(".wasm") {
                continue;
            }

            // Read in the bytes of the module
            let module_bytes = match std::fs::read(path.path()) {
                Ok(b) => b,
                _ => continue,
            };

            (filename, module_bytes)
        } else {
            continue;
        };

        // See if a type is defined in the configuration file, if not then we will grab the first part
        // of the filename up to the first underscore.
        let type_ = if let Some(type_) = config.log_type_overrides.get(&filename) {
            type_.to_string()
        } else {
            let type_: Vec<&str> = filename.split('_').collect();
            type_[0].to_string()
        };

        // Get the computation limit for the module by checking the following in order:
        // Module Override
        // Log Type amount
        // Default amount
        let computation_limit = match (
            config
                .computation_amount
                .module_overrides
                .get(&filename.to_string()),
            config.computation_amount.log_type.get(&type_),
            config.computation_amount.default,
        ) {
            (Some(amount), _, _) => *amount,
            (None, Some(amount), _) => *amount,
            (None, None, amount) => amount,
        };

        // Get the memory limit for the module by checking the following in order:
        // Module Override
        // Log Type amount
        // Default amount
        let page_count = match (
            config
                .memory_page_count
                .module_overrides
                .get(&filename.to_string()),
            config.memory_page_count.log_type.get(&type_),
            config.memory_page_count.default,
        ) {
            (Some(amount), _, _) => *amount,
            (None, Some(amount), _) => *amount,
            (None, None, amount) => amount,
        };

        // Page count is at max 32 bits. Nothing should ever allocate that many pages
        // but we're likely to hit this if someone spams the number key on their keyboard
        // for "unlimited memory".
        let page_count = if page_count > u32::MAX as u64 {
            u32::MAX
        } else {
            page_count as u32
        };

        let metering = Arc::new(Metering::new(computation_limit, cost_function));
        let mut compiler = Cranelift::default();
        compiler.push_middleware(metering);

        let base = BaseTunables::for_target(&Target::default());
        let tunables = LimitingTunables::new(base, Pages(page_count));
        let mut engine: Engine = compiler.into();
        engine.set_tunables(tunables);

        info!("Name: [{filename}] Computation Limit: [{computation_limit}] Memory Limit: [{page_count} pages] Log Type: [{type_}]");

        let cache = match config.lru_cache_size {
            None | Some(0) => None,
            Some(size) => Some(Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(size).unwrap(),
            )))),
        };

        let mut module = Module::new(&engine, module_bytes).unwrap();
        module.set_name(&filename);
        for import in module.imports() {
           info!("\tImport: {}", import.name());
        }

        let plaid_module = PlaidModule {
            computation_limit,
            page_limit: page_count,
            secrets: byte_secrets.get(&type_).map(|x| x.clone()),
            cache,
            name: filename.clone(),
            module,
            engine,
        };

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
