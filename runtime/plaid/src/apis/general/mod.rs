mod logback;
mod network;
mod random;

use crossbeam_channel::Sender;
use reqwest::{redirect, Client};
use ring::rand::SystemRandom;
use serde::Deserialize;
use tokio::sync::Mutex;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::ring::default_provider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, DigitallySignedStruct, Error as RustlsError, RootCertStore, SignatureScheme,
};

use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

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
    /// stored certs
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

        let specialized = config
            .network
            .web_requests
            .iter()
            .filter_map(|(name, req)| {
                // An MNR needs a specialized client if it specifies
                // * a custom timeout
                // * a custom root CA
                // * that it allows redirects
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
                    if req.return_cert_chain {
                        // set capturing verifier
                    }

                    let client = builder.build().unwrap();
                    Some((name.clone(), client))
                } else {
                    None
                }
            })
            .collect::<HashMap<String, Client>>();

        let captured_certs = Arc::new(Mutex::new(Option::None));

        Self {
            default,
            specialized,
            captured_certs,
        }
    }
}

pub fn get_verifier() -> Result<(), Box<dyn Error>> {
    todo!()
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

fn der_to_pem(der: &[u8]) -> String {
    let b64 = base64::encode(der);
    let mut pem = String::new();
    pem.push_str("-----BEGIN CERTIFICATE-----\n");
    for (i, char) in b64.chars().enumerate() {
        pem.push(char);
        if (i + 1) % 64 == 0 {
            pem.push('\n');
        }
    }
    if !b64.is_empty() && b64.len() % 64 != 0 {
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");
    pem
}

#[derive(Debug)]
struct CapturingVerifier {
    inner: Arc<WebPkiServerVerifier>,
    captured_chain: Arc<Mutex<Option<Vec<Vec<u8>>>>>,
}

impl ServerCertVerifier for CapturingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        // Capture the entire chain: end_entity + intermediates
        let mut chain: Vec<Vec<u8>> = Vec::with_capacity(1 + intermediates.len());
        chain.push(end_entity.as_ref().to_vec());
        for intermediate in intermediates {
            chain.push(intermediate.as_ref().to_vec());
        }
        *self.captured_chain.try_lock().unwrap() = Some(chain);

        // Delegate to the inner verifier
        self.inner
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn run() -> Result<(), Box<dyn std::error::Error>> {
        // Set up root certificates using webpki-roots
        let mut root_store = RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let crypto_provider = default_provider();
        let provider_arc = Arc::new(crypto_provider);

        // Create the default verifier with explicit provider
        let default_verifier = WebPkiServerVerifier::builder_with_provider(
            Arc::new(root_store.clone()),
            provider_arc.clone(),
        )
        .build()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        // Shared storage for the captured certificate chain (Vec<Vec<u8>> for DER bytes)
        let captured_chain: Arc<Mutex<Option<Vec<Vec<u8>>>>> = Arc::new(Mutex::new(None));

        // Custom verifier that captures the entire chain
        let custom_verifier = CapturingVerifier {
            inner: default_verifier,
            captured_chain: captured_chain.clone(),
        };

        // Build the ClientConfig with the custom verifier
        let mut config = ClientConfig::builder_with_provider(provider_arc)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
            .with_root_certificates(root_store)
            .with_no_client_auth();

        config
            .dangerous()
            .set_certificate_verifier(Arc::new(custom_verifier));

        // Create the reqwest client with the custom TLS config
        let client = Client::builder()
            .use_rustls_tls()
            .use_preconfigured_tls(config)
            .build()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        // Make a request to capture the chain
        let response = client
            .get("https://chain.link")
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        println!("Response status: {}", response.status());

        // Retrieve the captured chain bytes
        let chain_bytes = captured_chain.try_lock().unwrap().take().ok_or_else(|| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "No chain captured",
            )) as Box<dyn std::error::Error>
        })?;

        // Convert each DER to PEM
        let chain_pem: Vec<String> = chain_bytes.iter().map(|bytes| der_to_pem(bytes)).collect();

        // Output the PEM chain
        println!("Certificate chain PEM:");
        for (i, pem) in chain_pem.iter().enumerate() {
            println!("Certificate {}:\n{}", i, pem);
        }

        Ok(())
    }
}
