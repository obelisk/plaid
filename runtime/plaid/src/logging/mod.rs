mod splunk;
mod webhook;

mod stdout;

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};

use serde::{Deserialize, Serialize};
use std::{
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio::runtime::Runtime;

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
        test_mode: bool,
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
    heartbeat_interval: Option<u64>,
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

/// Errors encountered while trying to log something.
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
    fn send_log(&self, log: &WrappedLog) -> Result<(), LoggingError>;
}

#[derive(Clone)]
pub struct Logger {
    sender: Sender<Log>,
}

impl Logger {
    pub fn log_ts(&self, measurement: String, value: i64) -> Result<(), LoggingError> {
        self.sender
            .send(Log::TimeseriesPoint { measurement, value })
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    pub fn log_function_call(
        &self,
        module: String,
        function: String,
        test_mode: bool,
    ) -> Result<(), LoggingError> {
        self.sender
            .send(Log::HostFunctionCall {
                module,
                function,
                test_mode,
            })
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    pub fn log_module_error(
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
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    pub fn log_internal_message(
        &self,
        severity: Severity,
        message: String,
    ) -> Result<(), LoggingError> {
        self.sender
            .send(Log::InternalMessage { severity, message })
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    pub fn log_websocket_dropped(&self, socket_name: String) -> Result<(), LoggingError> {
        self.sender
            .send(Log::WebSocketConnectionDropped { socket_name })
            .map_err(|_| LoggingError::LoggingSystemDead)
    }

    /// This is the entry point of our logging thread started from main. This
    /// should be running in its own thread waiting for logs to come in from
    /// the tonic server. If it does not receive a message in 300 seconds it
    /// will send a heartbeat message instead. For stdout, and influx, this is
    /// a noop and will not actually be sent to the backend (or logged to the
    /// screen).
    fn logging_thread_loop(
        config: LoggingConfiguration,
        log_receiver: Receiver<Log>,
    ) -> Result<(), LoggingError> {
        let runtime = Runtime::new().unwrap();
        let heartbeat_interval = config.heartbeat_interval.unwrap_or(300);
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
                Some(splunk::SplunkLogger::new(config, runtime.handle().clone()))
            }
            None => None,
        };

        let webhook_logger = match config.webhook {
            Some(config) => {
                info!("Configured logger: webhook");
                Some(webhook::WebhookLogger::new(
                    config,
                    runtime.handle().clone(),
                ))
            }
            None => None,
        };

        // Main logging loop
        loop {
            let log = match log_receiver.recv_timeout(Duration::from_secs(heartbeat_interval)) {
                Ok(l) => l,
                Err(RecvTimeoutError::Timeout) => Log::Heartbeat {},
                _ => break,
            };

            let log = WrappedLog {
                log,
                identifier: config.identifier.clone().unwrap_or_default(),
            };

            if let Some(logger) = &stdout_logger {
                logger.send_log(&log).unwrap();
            }

            if let Some(logger) = &splunk_logger {
                if let Err(_) = logger.send_log(&log) {
                    error!("Could not send logs to Splunk");
                }
            }

            if let Some(logger) = &webhook_logger {
                if let Err(_) = logger.send_log(&log) {
                    error!("Could not send logs to webhook");
                }
            }
        }

        error!("Logging thread has gone away.");
        Err(LoggingError::LoggingSystemDead)
    }

    pub fn start(config: LoggingConfiguration) -> (Self, JoinHandle<Result<(), LoggingError>>) {
        let (sender, rx) = bounded(4096);
        let _handle = thread::spawn(move || Self::logging_thread_loop(config, rx));

        (Self { sender }, _handle)
    }
}
