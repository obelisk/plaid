use clap::{Arg, ArgAction, Command};

use plaid_stl::messages::LogbacksAllowed;
use ring::digest::{self, digest};
use serde::{de, Deserialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::performance::PerformanceMonitoring;
use crate::InstanceRoles;

use super::apis::ApiConfigs;
use super::data::DataConfig;
use super::loader::Configuration as LoaderConfiguration;
use super::logging::LoggingConfiguration;
use super::storage::Config as StorageConfig;

/// How should responses to GET requests be cached.
#[derive(Default, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum CachingMode {
    /// Do not cache.
    #[default]
    None,
    /// Run then cache the result for the given period of time. The
    /// cache will be invalidated after the given period of time has
    /// passed.
    Timed { validity: u64 },
    /// This will use a stored cache from whatever the response method is
    /// before running the response method again. This means that GET calls
    /// should always be fast as the cache is only updated by other means.
    ///
    /// The exception is if call_on_none is set to true, in which case if
    /// nothing is stored, this will act as if Cache::None was set for the first
    /// call.
    UsePersistentResponse { call_on_none: bool },
}

/// How should a webhook respond to a GET request
#[derive(Clone)]
pub enum ResponseMode {
    /// Respond the way Facebook expects, needs a secret token
    Facebook(String),
    /// Respond by running a plaid wasm module to generate the response
    Rule(String),
    /// Static response
    Static(String),
}

/// Some services have verification routines that need to happen before
/// data will be sent to them. To make the system more general you can
/// specify on each webhook what kind of response you'd like to use
/// to pass this verification.
///
/// This configuration controls how a webserver will respond to GET
/// requests.
#[derive(Deserialize, Clone)]
pub struct GetMode {
    /// Set how the data sent in GET responses should be cached. This is really
    /// only useful when the response_mode is set to ResponseMode::Rule but in future
    /// this may be applicable to other, newer, modes.
    #[serde(default)]
    pub caching_mode: CachingMode,
    /// How the webhook should respond to a GET request
    #[serde(deserialize_with = "response_mode_deserializer")]
    pub response_mode: ResponseMode,
}

/// Configuration for a particular webhook within a WebhookServer to accept
/// logs and send them to a logging channel
#[derive(Deserialize, Clone)]
pub struct WebhookConfig {
    /// The logging channel that POST bodies will be sent to
    pub log_type: String,
    /// What headers do you want forwarded to the logging channel
    pub headers: Vec<String>,
    /// See GetMode
    pub get_mode: Option<GetMode>,
    /// An optional label for the webhook. If this is populated, it will be
    /// passed as the source to to the modules instead of the webhook address.
    /// You may want to do this to reduce the secrets modules have access to.
    pub label: Option<String>,
    /// The maximum number of logbacks that each rule will be allowed to trigger
    /// per message received. If this is set to Limited(0), no rule will be able to use the log
    /// back functionality from messages generated by this webhook. If this is set to Limited(1),
    /// then EACH RULE will be able to trigger one logback. If this is set to Unlimited, then
    /// each rule will be able to trigger as many logbacks as they want (and those triggered rules
    /// will be able to as well). If this is not set, it will default to Limited(0).
    #[serde(default)]
    pub logbacks_allowed: LogbacksAllowed,
}

/// Configuration for a webhook server
#[derive(Deserialize)]
pub struct WebhookServerConfiguration {
    /// The address and port to listen on for webhooks
    pub listen_address: String,
    /// The mapping of webhooks to configuration of the webhook
    #[serde(default)]
    pub webhooks: HashMap<String, WebhookConfig>,
}

/// Configuration for a thread pool / channel dedicated to a log type
#[derive(Deserialize)]
pub struct DedicatedThreadsConfig {
    pub num_threads: u8,
    #[serde(default = "default_log_queue_size")]
    pub log_queue_size: usize,
}

/// The configuration for the executor system
#[derive(Deserialize)]
pub struct ExecutorConfig {
    /// How many threads should be used for executing modules when logs come in
    ///
    /// Modules do not get more than one thread, this just means that modules can
    /// execute in parallel
    pub execution_threads: u8,
    /// The maximum number of logs in the queue to be processed at once
    #[serde(default = "default_log_queue_size")]
    pub log_queue_size: usize,
    /// Number of threads dedicated to specific log types.
    /// This is a mapping {log type --> num threads}.
    #[serde(default)]
    pub dedicated_threads: HashMap<String, DedicatedThreadsConfig>,
}

/// The full configuration of Plaid
#[derive(Deserialize)]
pub struct Configuration {
    /// How APIs are configured. These APIs are accessible to modules
    /// so they can take advantage of Plaid abstractions
    pub apis: ApiConfigs,
    /// Data generators. These are systems that pull data directly rather
    /// than waiting for data to come in via Webhook
    pub data: DataConfig,
    /// The executor subsystem.
    pub executor: ExecutorConfig,
    /// Configuration for how Plaid monitors rule performance. When enabled,
    /// Plaid outputs a metrics file with performance metadata for all
    /// rules than have been run at least once.
    pub performance_monitoring: Option<PerformanceMonitoring>,
    /// Configuration for persistent data. This allows modules to store data between
    /// invocations
    pub storage: Option<StorageConfig>,
    /// The external logging system. This allows you to send data to external systems
    /// for monitoring
    pub logging: LoggingConfiguration,
    /// See WebhookServerConfiguration
    pub webhooks: HashMap<String, WebhookServerConfiguration>,
    /// Set what modules will be loaded, what logging channels they're going to use
    /// and their computation and memory limits.
    pub loading: LoaderConfiguration,
}

/// Plaid's configuration augmented with the roles that this instance is playing.
pub struct ConfigurationWithRoles {
    /// Plaid's configuration
    pub config: Configuration,
    /// The roles that this instance has, i.e., what this instance is running
    pub roles: InstanceRoles,
}

/// This function provides the default log queue size in the event that one isn't provided
fn default_log_queue_size() -> usize {
    2048
}

/// All errors that can be encountered while configuring Plaid
#[derive(Debug)]
pub enum ConfigurationError {
    FileError,
    ParsingError,
    ComputationLimitInvalid,
    ExecutionThreadsInvalid,
}

impl std::fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigurationError::FileError => write!(
                f,
                "There was an error finding or reading the configuration file"
            ),
            ConfigurationError::ParsingError => {
                write!(f, "The format of the configuration file was incorrect")
            }
            ConfigurationError::ComputationLimitInvalid => {
                write!(f, "The computation limit must be non zero")
            }
            ConfigurationError::ExecutionThreadsInvalid => {
                write!(
                    f,
                    "The number of execution threads must be between 1 and 255"
                )
            }
        }
    }
}

impl std::error::Error for ConfigurationError {}

/// Deserialized for a webhook's response mode
fn response_mode_deserializer<'de, D>(deserializer: D) -> Result<ResponseMode, D::Error>
where
    D: de::Deserializer<'de>,
{
    let mode = String::deserialize(deserializer)?;

    let mut pieces: Vec<&str> = mode.split(":").collect();

    let data = pieces.pop().ok_or(serde::de::Error::custom(
        "Must provide context for the response_mode. For Facebook/Meta this is the secret, for Rule this is the module name",
    ))?;

    let mode = pieces
        .pop()
        .ok_or(serde::de::Error::custom("Must provide a response_mode"))?;

    Ok(match mode {
        "facebook" | "meta" => ResponseMode::Facebook(data.to_owned()),
        "Rule" | "rule" => ResponseMode::Rule(data.to_owned()),
        "Static" | "static" => ResponseMode::Static(data.to_owned()),
        x => {
            return Err(serde::de::Error::custom(format!(
                "{x} is an unknown response_mode. Must be 'facebook', 'rule', or 'static'"
            )))
        }
    })
}

/// Configure Plaid with config file and secrets read from arguments (or use default values).
pub fn configure() -> Result<ConfigurationWithRoles, ConfigurationError> {
    let matches = Command::new("Plaid - A sandboxed automation engine")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Mitchell Grenier <mitchell@confurious.io>")
        .about("Write security rules in anything that compiles to WASM, run them with only the access they need.")
        .arg(
            Arg::new("config")
                .help("Path to the folder with configuration toml files")
                .long("config")
                .default_value("./plaid/resources/config")
        )
        .arg(
            Arg::new("secrets")
                .help("Path to the secrets file")
                .long("secrets")
                .default_value("./plaid/private-resources/secrets.toml")
        )
        .arg(Arg::new("no_wh").help("Do not run webhooks").long("no-webhooks").action(ArgAction::SetTrue))
        .arg(Arg::new("no_dg").help("Do not run data generators").long("no-data-generators").action(ArgAction::SetTrue))
        .arg(Arg::new("no_int").help("Do not run interval jobs").long("no-interval-jobs").action(ArgAction::SetTrue))
        .arg(Arg::new("no_lb").help("Do not run logbacks").long("no-logbacks").action(ArgAction::SetTrue))
        .arg(Arg::new("no_nonconc").help("Do not run non-concurrent log types").long("no-non-concurrent").action(ArgAction::SetTrue))
        .get_matches();

    let config_folder = matches.get_one::<String>("config").unwrap();
    let secrets_path = matches.get_one::<String>("secrets").unwrap();

    let no_webhooks = matches.get_one::<bool>("no_wh").unwrap();
    let no_data_generators = matches.get_one::<bool>("no_dg").unwrap();
    let no_interval_jobs = matches.get_one::<bool>("no_int").unwrap();
    let no_logbacks = matches.get_one::<bool>("no_lb").unwrap();
    let no_nonconcurrent = matches.get_one::<bool>("no_nonconc").unwrap();

    let config = read_and_interpolate(config_folder, secrets_path, false)?;

    let roles = InstanceRoles {
        webhooks: !*no_webhooks,
        data_generators: !*no_data_generators,
        interval_jobs: !*no_interval_jobs,
        logbacks: !*no_logbacks,
        non_concurrent_rules: !*no_nonconcurrent,
    };

    Ok(ConfigurationWithRoles { config, roles })
}

/// Reads configuration files from a given folder and a secrets file, concatenates the config files into one,
/// interpolates the secrets into the configuration, and parses the result into a `Configuration` struct.
pub fn read_and_interpolate(
    config_folder: &str,
    secrets_path: &str,
    show_config: bool,
) -> Result<Configuration, ConfigurationError> {
    // Read the configuration files from a given folder, and concatenate them into one
    let mut config = String::new();

    let entries = match std::fs::read_dir(config_folder) {
        Ok(x) => x,
        Err(e) => {
            error!("Encountered error when trying to read configuration folder! Error: {e}");
            return Err(ConfigurationError::FileError);
        }
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|dir_entry| dir_entry.path())
        .filter(|path| path.is_file())
        .collect();
    paths.sort_by(|a, b| {
        a.file_name()
            .and_then(|a_name| b.file_name().map(|b_name| a_name.cmp(b_name)))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for path in paths {
        match std::fs::read_to_string(&path) {
            Ok(content) => config.push_str(&content),
            Err(e) => {
                error!("Encountered error when trying to read configuration! Error: {e}");
                return Err(ConfigurationError::FileError);
            }
        };
    }

    // Read the secrets file and parse into a TOML object
    let secrets = std::fs::read_to_string(secrets_path)
        .map_err(|e| {
            error!("Encountered file error when trying to read secrets file! Error: {e}");
            ConfigurationError::FileError
        })
        .and_then(|s| {
            toml::from_str::<toml::value::Table>(&s).map_err(|e| {
                error!(
                    "Encountered error when trying to parse secrets into a TOML table! Error: {e}"
                );
                ConfigurationError::FileError
            })
        })?;

    // Iterate over the secrets we just parsed and replace matching keys in the config
    for (secret, value) in secrets {
        config = config.replace(
            &format!("{{plaid-secret{{{secret}}}}}"), // this means {plaid-secret{secret}}
            value.as_str().unwrap(),
        );
    }

    if show_config {
        println!("---------- Plaid Config ----------\n{config}");
        let config_hash = digest(&digest::SHA256, config.as_bytes())
            .as_ref()
            .iter()
            .map(|byte| format!("{:02x}", byte))
            .collect::<String>();
        println!("---------- Configuration Hash ----------\n{config_hash}")
    }

    // Parse the TOML into our configuration structures
    let config: Configuration = match toml::from_str(&config) {
        Ok(config) => config,
        Err(e) => {
            error!("Encountered parsing error while reading configuration with interpolated secrets! Error: {e}");
            return Err(ConfigurationError::ParsingError);
        }
    };

    if config.executor.execution_threads == 0 {
        return Err(ConfigurationError::ExecutionThreadsInvalid);
    }

    Ok(config)
}
