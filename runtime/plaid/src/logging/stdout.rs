//! This module provides a way for Plaid to log to stdout.

use super::{Log, LoggingError, PlaidLogger, Severity, WrappedLog};

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {}

pub struct StdoutLogger {}

impl StdoutLogger {
    pub fn new(_config: Config) -> Self {
        Self {}
    }
}

impl PlaidLogger for StdoutLogger {
    fn send_log(&self, log: &WrappedLog) -> Result<(), LoggingError> {
        match &log.log {
            Log::InternalMessage { severity, message } => match severity {
                Severity::Error => error!("{}", message),
                Severity::Warning => warn!("{}", message),
                Severity::Info => info!("{}", message),
            },
            Log::HostFunctionCall {
                module,
                function,
                test_mode,
            } => {
                if *test_mode {
                    debug!("TEST MODE [{module}] is calling [{function}]")
                } else {
                    debug!("[{module}] is calling [{function}]")
                }
            }
            Log::ModuleExecutionError { module, error, log } => {
                debug!("[{module}] errored with error [{error}]. Provided Log: {log}")
            }
            Log::TimeseriesPoint { measurement, value } => {
                trace!("New TS Point: ({measurement}, {value})")
            }
            Log::WebSocketConnectionDropped { socket_name } => {
                warn!("Connection to socket: {socket_name} dropped unexpectedly");
            }
            Log::Heartbeat { .. } => (),
        }
        Ok(())
    }
}
