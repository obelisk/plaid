mod splunk;
mod webhook;

mod stdout;

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::{
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
};

/// A severity scale to measure how critical a log is when sent
/// to a logging service.
#[derive(Serialize)]
pub enum Severity {
    /// An informative log
    #[allow(dead_code)]
    Info,
    /// A non critical error
    Warning,
    /// A critical error
    Error,
}

/// Represents a log to be sent to the configured logging systems.
#[derive(Serialize)]
pub enum Log {
    /// Used for relaying status messages to a logging backend. Plaid errors
    /// or failures send messages of this type.
    InternalMessage {
        severity: Severity,
        message: String,
    },
    HostFunctionCall {
        module: String,
        function: String,
    },
    TimeseriesPoint {
        measurement: String,
        value: i64,
    },
    ModuleExecutionError {
        module: String,
        error: String,
        log: String,
    },
    WebSocketConnectionDropped {
        socket_name: String,
    },
    /// Is not used by other components of Plaid. This is created and sent
    /// by the logging system if it has not received a message from the server
    /// module for a period of time.
    Heartbeat {},
}

/// Logs are public to the rest of the codebase so we have no control over their
/// contents. This type wraps those logs in an additional structure to allow us
/// to add metadata relevant to the logging system or instance itself.
#[derive(Serialize)]
struct WrappedLog {
    /// The log sent from the server module
    log: Log,
    /// An identifier to identify this instance or configuration in redundant
    /// environments
    identifier: String,
}

/// Defines the complete logging configuration shape. This consists of some top
/// level options for configuring logging as a whole, then several optional sub
/// structs that configure individual logging systems.
#[derive(Deserialize)]
pub struct LoggingConfiguration {
    /// This is used as a decorator when sending logs to backends in the event
    /// that there are multiple Plaid instances in a single logging
    /// environment.
    identifier: Option<String>,
    /// If logs aren't received after this many seconds, the system will send an
    /// empty heartbeat log to the logging systems to signal it is still up
    /// and healthy.
    #[serde(default = "default_log_heartbeat_interval")]
    #[serde(deserialize_with = "parse_heartbeat_interval")]
    heartbeat_interval: Duration,
    /// Configures the stdout logger. This is powered by env_logger and is a
    /// thin wrapper around it, however it lets us log to stdout the same way
    /// we log to other more complex systems.
    stdout: Option<stdout::Config>,
    /// Log to Splunk for standard logging. The splunk module contains more
    /// information on configuring this logger.
    splunk: Option<splunk::Config>,
    /// Log JSON to a POST endpoint. This is used for generic logging systems
    /// so it's easy to operate on Plaid events. It's likely in future the
    /// Splunk logger code will be a specific instantiation of this.
    webhook: Option<webhook::Config>,
}

/// Custom deserialized for `heartbeat_interval`.
fn parse_heartbeat_interval<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let interval = u64::deserialize(deserializer)?;
    Ok(Duration::from_secs(interval))
}

/// Default value for `hearbeat_interval` in `LoggingConfiguration` in the event
/// that one is not provided
fn default_log_heartbeat_interval() -> Duration {
    Duration::from_secs(300)
}

#[derive(Debug)]
pub enum LoggingError {
    #[allow(dead_code)]
    /// Returned when there is a failure serializing received logging data
    SerializationError(String),
    #[allow(dead_code)]
    /// Returned when there is a issue communicating with a backend or other
    /// remote system.
    CommunicationError(String),
    /// Logging system has gone away
    LoggingSystemDead,
}

impl std::fmt::Display for LoggingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoggingError::SerializationError(e) => write!(f, "Logging serialization error: {}", e),
            LoggingError::CommunicationError(e) => write!(f, "Logging communication error: {}", e),
            LoggingError::LoggingSystemDead => write!(f, "Logging system has gone away"),
        }
    }
}

/// To implement a new logger, it must implement the `send_log` function
/// and return success or failure.
trait PlaidLogger {
    async fn send_log(&self, log: &WrappedLog) -> Result<(), LoggingError>;
}

#[derive(Clone)]
pub struct Logger {
    sender: Sender<Log>,
}

impl Logger {
    /// Logs a timeseries point asynchronously
    pub async fn log_ts(&self, measurement: String, value: i64) -> Result<(), LoggingError> {
        self.sender
            .send(Log::TimeseriesPoint { measurement, value })
            .await
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// Logs a timeseries point synchronously
    /// This function should be called when logging from a synchronous context
    pub fn log_ts_sync(&self, measurement: String, value: i64) -> Result<(), LoggingError> {
        self.sender
            .blocking_send(Log::TimeseriesPoint { measurement, value })
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// Logs a module function call asynchronously
    pub async fn log_function_call(
        &self,
        module: String,
        function: String,
    ) -> Result<(), LoggingError> {
        self.sender
            .send(Log::HostFunctionCall { module, function })
            .await
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// Logs a module function call synchronously
    /// This function should be called when logging from a synchronous context
    pub fn log_function_call_sync(
        &self,
        module: String,
        function: String,
    ) -> Result<(), LoggingError> {
        self.sender
            .blocking_send(Log::HostFunctionCall { module, function })
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// Logs a module execution error asynchronously.
    pub async fn log_module_error(
        &self,
        module: String,
        error: String,
        log: Vec<u8>,
    ) -> Result<(), LoggingError> {
        let log = match String::from_utf8(log.clone()) {
            Ok(log) => log,
            Err(_) => base64::encode(log),
        };
        self.sender
            .send(Log::ModuleExecutionError { module, error, log })
            .await
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// Logs a module execution error synchronously.
    /// /// This function should be called when logging from a synchronous context
    pub fn log_module_error_sync(
        &self,
        module: String,
        error: String,
        log: Vec<u8>,
    ) -> Result<(), LoggingError> {
        let log = match String::from_utf8(log.clone()) {
            Ok(log) => log,
            Err(_) => base64::encode(log),
        };
        self.sender
            .blocking_send(Log::ModuleExecutionError { module, error, log })
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// Logs a message asyncronously
    pub async fn log_internal_message(
        &self,
        severity: Severity,
        message: String,
    ) -> Result<(), LoggingError> {
        self.sender
            .send(Log::InternalMessage { severity, message })
            .await
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// Logs a message syncronously
    /// This function should be called when logging from a synchronous context
    pub fn log_internal_message_sync(
        &self,
        severity: Severity,
        message: String,
    ) -> Result<(), LoggingError> {
        self.sender
            .blocking_send(Log::InternalMessage { severity, message })
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// Logs a websocket dropped event asyncronously
    pub async fn log_websocket_dropped(&self, socket_name: String) -> Result<(), LoggingError> {
        self.sender
            .send(Log::WebSocketConnectionDropped { socket_name })
            .await
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// Logs a websocket dropped event synchronous.
    /// This function should be called when logging from a synchronous context
    pub fn log_websocket_dropped_sync(&self, socket_name: String) -> Result<(), LoggingError> {
        self.sender
            .blocking_send(Log::WebSocketConnectionDropped { socket_name })
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// This is the entry point of our logging thread started from main. This
    /// should be running in its own thread waiting for logs to come in from
    /// the tonic server. If it does not receive a message in 300 seconds it
    /// will send a heartbeat message instead. For stdout, and influx, this is
    /// a noop and will not actually be sent to the backend (or logged to the
    /// screen).
    async fn logging_thread_loop(
        config: LoggingConfiguration,
        log_receiver: &mut Receiver<Log>,
    ) -> Result<(), LoggingError> {
        // Configure the different loggers
        let stdout_logger = match config.stdout {
            Some(config) => {
                info!("Configured logger: stdout");
                Some(stdout::StdoutLogger::new(config))
            }
            None => {
                println!("stdout logger is not enabled. This is not recommended!");
                None
            }
        };

        let splunk_logger = match config.splunk {
            Some(config) => {
                info!("Configured logger: splunk");
                Some(splunk::SplunkLogger::new(config))
            }
            None => None,
        };

        let webhook_logger = match config.webhook {
            Some(config) => {
                info!("Configured logger: webhook");
                Some(webhook::WebhookLogger::new(config))
            }
            None => None,
        };

        // Main logging loop
        loop {
            let log =
                match tokio::time::timeout(config.heartbeat_interval, log_receiver.recv()).await {
                    Ok(Some(log)) => log,
                    Ok(None) => break,
                    Err(_) => Log::Heartbeat {},
                };

            let log = WrappedLog {
                log,
                identifier: config.identifier.clone().unwrap_or_default(),
            };

            if let Some(logger) = &stdout_logger {
                logger.send_log(&log);
            }

            if let Some(logger) = &splunk_logger {
                if let Err(_) = logger.send_log(&log).await {
                    error!("Could not send logs to Splunk");
                }
            }

            if let Some(logger) = &webhook_logger {
                if let Err(_) = logger.send_log(&log).await {
                    error!("Could not send logs to webhook");
                }
            }
        }

        error!("Logging thread has gone away.");
        Err(LoggingError::LoggingSystemDead)
    }

    pub fn start(config: LoggingConfiguration) -> (Self, JoinHandle<Result<(), LoggingError>>) {
        let (sender, mut rx) = tokio::sync::mpsc::channel(4096);
        let _handle =
            tokio::task::spawn(async move { Self::logging_thread_loop(config, &mut rx).await });

        (Self { sender }, _handle)
    }
}
