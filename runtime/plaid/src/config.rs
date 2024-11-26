use clap::{Arg, Command};

use ring::digest::{self, digest};
use serde::Deserialize;
use serde_json::Value;

use crate::performance::PerformanceMonitoring;

use super::apis::Apis;
use super::data::DataConfig;
use super::loader::Configuration as LoaderConfiguration;
use super::logging::LoggingConfiguration;
use super::storage::Config as StorageConfig;

/// The full configuration of Plaid
#[derive(Deserialize)]
pub struct Configuration {
    /// How APIs are configured. These APIs are accessible to modules
    /// so they can take advantage of Plaid abstractions
    pub apis: Apis,
    /// Data generators. These are systems that pull data directly rather
    /// than waiting for data to come in via Webhook
    pub data: DataConfig,
    /// How many threads should be used for executing modules when logs come in
    ///
    /// Modules do not get more than one thread, this just means that modules can
    /// execute in parallel
    pub execution_threads: u8,
    /// The maximum number of logs in the queue to be processed at once
    #[serde(default = "default_log_queue_size")]
    pub log_queue_size: usize,
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
    /// Set what modules will be loaded, what logging channels they're going to use
    /// and their computation and memory limits.
    pub loading: LoaderConfiguration,
}

/// This function provides the default log queue size in the event that one isn't provided
fn default_log_queue_size() -> usize {
    2048
}

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

pub fn configure() -> Result<Configuration, ConfigurationError> {
    let matches = Command::new("Plaid - A sandboxed automation engine")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Mitchell Grenier <mitchell@confurious.io>")
        .about("Write security rules in anything that compiles to WASM, run them with only the access they need.")
        .arg(
            Arg::new("config")
                .help("Path to the configuration toml file")
                .long("config")
                .default_value("./plaid/resources/plaid.toml")
        )
        .arg(
            Arg::new("secrets")
                .help("Path to the secrets json file")
                .long("secrets")
                .default_value("./plaid/private-resources/secrets.json")
        ).get_matches();

    let config_path = matches.get_one::<String>("config").unwrap();
    let secrets_path = matches.get_one::<String>("secrets").unwrap();

    read_and_interpolate(config_path, secrets_path, false)
}

/// Reads a configuration file and a secrets file, interpolates the secrets into the configuration,
/// and parses the result into a `Configuration` struct.
pub fn read_and_interpolate(
    config_path: &str,
    secrets_path: &str,
    show_config: bool,
) -> Result<Configuration, ConfigurationError> {
    // Read the configuration file
    let mut config = match std::fs::read_to_string(config_path) {
        Ok(config) => config,
        Err(e) => {
            error!("Encountered file error when trying to read configuration!. Error: {e}");
            return Err(ConfigurationError::FileError);
        }
    };

    // Read the secrets file and parse into a serde object
    let secrets = match std::fs::read(secrets_path) {
        Ok(secret_bytes) => {
            let secrets = serde_json::from_slice::<Value>(&secret_bytes).unwrap();
            secrets.as_object().cloned().unwrap()
        }
        Err(e) => {
            error!("Encountered file error when trying to read secrets file!. Error: {e}");
            return Err(ConfigurationError::FileError);
        }
    };

    // Iterate over the secrets we just parsed and replace matching keys in the config
    for (secret, value) in secrets {
        config = config.replace(&secret, value.as_str().unwrap());
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
            error!("Encountered parsing error while reading configuration with interpolated secrets!. Error: {e}");
            return Err(ConfigurationError::ParsingError);
        }
    };

    if config.execution_threads == 0 {
        return Err(ConfigurationError::ExecutionThreadsInvalid);
    }

    Ok(config)
}
