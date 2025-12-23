mod cert_sni;
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
    /// The default `Client` used for requests without customizations.
    default: Client,
    /// Named `Client` instances where special configurations have been applied.
    specialized: HashMap<String, Client>,
}

/// An MNR needs a specialized client if it specifies
/// * a custom timeout
/// * a custom root CA
/// * a permissive redirect policy
fn create_specialized_client(
    name: String,
    req: &network::Request,
    default_timeout_duration: Duration,
) -> Option<(String, Client)> {
    // If no specializations are needed, return None immediately
    if req.timeout.is_none()
        && req.root_certificate.is_none()
        && !req.enable_redirects
        && !req.return_cert
    {
        return None;
    }

    // If specializations are needed, start with the default timeout
    let mut builder =
        reqwest::Client::builder().timeout(req.timeout.unwrap_or(default_timeout_duration));

    // If the request has a custom root CA, then we need to add that into
    // the root certificate store
    if let Some(ref ca) = req.root_certificate {
        builder = builder.add_root_certificate(ca.clone());
    }

    if req.return_cert {
        // Enable certificate retrieval
        builder = builder.tls_info(true);
    }

    // All requests to follow redirects if needed. This is generally
    // not advised.
    builder = builder.redirect({
        if req.enable_redirects {
            redirect::Policy::default()
        } else {
            redirect::Policy::none()
        }
    });

    let client = builder.build().unwrap();
    Some((name.clone(), client))
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
                create_specialized_client(name.clone(), req, default_timeout_duration)
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
