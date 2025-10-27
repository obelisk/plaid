mod logback;
mod network;
mod random;

use crossbeam_channel::Sender;
use reqwest::{redirect, Client};

use ring::rand::SystemRandom;
use serde::Deserialize;

use std::{collections::HashMap, time::Duration};

use crate::{data::DelayedMessage, executor::Message};

use super::default_timeout_seconds;

#[derive(Deserialize)]
pub struct GeneralConfig {
    /// Configuration for network requests
    pub network: network::Config,
    /// The number of seconds until an external API request times out.
    /// If no value is provided, the result of `default_timeout_seconds()` will be used.
    #[serde(default = "default_timeout_seconds")]
    api_timeout_seconds: u64,
}

pub struct General {
    /// General Plaid configuration
    config: GeneralConfig,
    /// Client to make requests with
    clients: Clients,
    /// Sender object for messages
    log_sender: Sender<Message>,
    /// Sender object for messages that must be processed with a delay
    delayed_log_sender: Sender<DelayedMessage>,
    /// Secure random generator
    system_random: SystemRandom,
}

/// Holds the default HTTP client plus any named clients with per-request customizations.
pub struct Clients {
    /// The default `Client` used for requests without custom timeouts or certificates.
    default: Client,
    /// Named `Client` instances configured with custom timeouts or root certificates.
    specialized: HashMap<String, Client>,
}

impl Clients {
    fn new(config: &GeneralConfig) -> Self {
        let default_timeout_duration = Duration::from_secs(config.api_timeout_seconds);
        let default = reqwest::Client::builder()
            .timeout(default_timeout_duration)
            .redirect(redirect::Policy::none()) // by default, no redirects
            .build()
            .unwrap();

        let specialized = config
            .network
            .web_requests
            .iter()
            .filter_map(|(name, req)| {
                // An MNR needs a specialized client if it specifies
                // * a custom timeout
                // * a custom root CA
                // * that it allows redirects
                if req.timeout.is_some() || req.root_certificate.is_some() || req.enable_redirects {
                    let mut builder = reqwest::Client::builder()
                        .timeout(req.timeout.unwrap_or(default_timeout_duration));

                    if let Some(ca) = req.root_certificate.clone() {
                        builder = builder.add_root_certificate(ca);
                    }

                    // See if redirects should be enabled
                    builder = builder.redirect({
                        if req.enable_redirects {
                            redirect::Policy::default()
                        } else {
                            redirect::Policy::none()
                        }
                    });

                    let client = builder.build().unwrap();
                    Some((name.clone(), client))
                } else {
                    None
                }
            })
            .collect::<HashMap<String, Client>>();

        Self {
            default,
            specialized,
        }
    }
}

impl General {
    pub fn new(
        config: GeneralConfig,
        log_sender: Sender<Message>,
        delayed_log_sender: Sender<DelayedMessage>,
    ) -> Self {
        let clients = Clients::new(&config);
        let system_random = SystemRandom::new();

        Self {
            config,
            clients,
            log_sender,
            delayed_log_sender,
            system_random,
        }
    }
}
