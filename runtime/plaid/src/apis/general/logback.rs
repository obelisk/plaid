use crate::{data::DelayedMessage, executor::Message};
use plaid_stl::messages::{LogSource, LogbacksAllowed};

use super::General;

impl General {
    /// Send a log from a module back into the logging system into a particular
    /// type. You need to be very careful when allowing modules to use this
    /// because it can be used to trigger other rules with greater access than
    /// the calling module has.
    pub fn log_back(
        &self,
        type_: &str,
        log: &[u8],
        module: &str,
        delay: u64,
        logbacks_allowed: LogbacksAllowed,
    ) -> bool {
        let msg = Message::new(
            type_.to_string(),
            log.to_vec(),
            LogSource::Logback(module.to_string()),
            logbacks_allowed,
        );

        // Send the message to the dedicated channel, from where it will
        // be picked up by the dedicated data generator.
        self.delayed_log_sender
            .send(DelayedMessage::new(delay, msg))
            .is_ok()
    }
}
