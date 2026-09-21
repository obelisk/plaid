use serde::{Deserialize, Serialize};

/// Runtime-internal system functions that can trigger a log.
///
/// Rules match on these variants instead of magic strings, so a typo is a
/// compile error rather than a silently unmatched log.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemFunction {
    /// The transaction-confirmation poller.
    ConfirmTransaction,
}

impl std::fmt::Display for SystemFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemFunction::ConfirmTransaction => write!(f, "confirm_transaction"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Generator {
    Github,
    Okta,
    Interval(String),
    SQS(String),
    WebSocketExternal(String),
}

impl std::fmt::Display for Generator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Generator::Github => write!(f, "github"),
            Generator::Okta => write!(f, "okta"),
            Generator::Interval(job) => write!(f, "interval/{job}"),
            Generator::SQS(name) => write!(f, "sqs/{name}"),
            Generator::WebSocketExternal(ws) => write!(f, "websocket/{ws}"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogSource {
    Generator(Generator),
    WebhookPost(String),
    WebhookGet(String),
    Logback(String),
    /// A log produced by a runtime-internal system function, such as the
    /// transaction-confirmation poller.
    System(SystemFunction),
}

/// Represents how many logbacks can be triggered by the module that handles a message.
/// This can be a finite value (u32, with 0 a valid value) or it can be unlimited.
/// These are the TOML encodings for the two cases:
/// * "Unlimited"
/// * { Limited = value }
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogbacksAllowed {
    Unlimited,
    Limited(u32),
}

impl Default for LogbacksAllowed {
    fn default() -> Self {
        Self::Limited(0)
    }
}

impl std::fmt::Display for LogSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogSource::Generator(g) => write!(f, "generator/{g}"),
            LogSource::WebhookPost(w) => write!(f, "webhookpost/{w}"),
            LogSource::WebhookGet(w) => write!(f, "webhookget/{w}"),
            LogSource::Logback(m) => write!(f, "logback/{m}"),
            LogSource::System(s) => write!(f, "system/{s}"),
        }
    }
}
