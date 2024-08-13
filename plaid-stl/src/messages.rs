use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Generator {
    Github,
    Okta,
    Interval(String),
    Websocket(String),
}

impl std::fmt::Display for Generator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Generator::Github => write!(f, "github"),
            Generator::Okta => write!(f, "okta"),
            Generator::Interval(job) => write!(f, "interval/{job}"),
            Generator::Websocket(ws) => write!(f, "websocket/{ws}"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogSource {
    Generator(Generator),
    WebhookPost(String),
    WebhookGet(String),
    Logback(String),
}

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
        }
    }
}
