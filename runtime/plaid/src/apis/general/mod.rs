mod certs;
mod logback;
mod network;
mod random;

use crossbeam_channel::Sender;
use reqwest::{redirect, Client};
use ring::rand::SystemRandom;
use serde::Deserialize;
use tokio::sync::Mutex;

use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::apis::ApiError;
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
    /// Captured certificate chain from the server in DER format
    captured_certs: Arc<Mutex<Option<Vec<Vec<u8>>>>>,
}

impl Clients {
    fn new(config: &GeneralConfig) -> Self {
        let default_timeout_duration = Duration::from_secs(config.api_timeout_seconds);
        let default = reqwest::Client::builder()
            .timeout(default_timeout_duration)
            .redirect(redirect::Policy::none()) // by default, no redirects
            .build()
            .unwrap();

        let captured_certs = Arc::new(Mutex::new(Option::None));
        let specialized = config
            .network
            .web_requests
            .iter()
            .filter_map(|(name, req)| {
                // An MNR needs a specialized client if it specifies
                // * a custom timeout
                // * a custom root CA
                // * that it allows redirects
                // * capturing the server certificate chain
                if req.timeout.is_some()
                    || req.root_certificate.is_some()
                    || req.enable_redirects
                    || req.return_cert_chain
                {
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

                    // return cert chain
                    builder = if req.return_cert_chain {
                        // build custom tls config with capturing verifier
                        let config =
                            certs::capturing_verifier_tls_config(captured_certs.clone()).unwrap();

                        // set custom tls config on client
                        builder.use_rustls_tls().use_preconfigured_tls(config)
                    } else {
                        builder
                    };

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
            captured_certs,
        }
    }

    pub fn get_captured_certs(&self) -> Result<Option<Vec<String>>, ApiError> {
        let certs = self.captured_certs.try_lock().map_err(|err| {
            warn!("get_captured_certs try_lock failed {err}");
            ApiError::ImpossibleError
        })?;

        if let Some(chain_bytes) = &*certs {
            // Convert each DER to PEM
            let chain_pem: Vec<String> = chain_bytes
                .iter()
                .map(|bytes| certs::der_to_pem(bytes))
                .collect();

            Ok(Some(chain_pem))
        } else {
            Ok(None)
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
