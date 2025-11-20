use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::ring::default_provider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, DigitallySignedStruct, Error as RustlsError, RootCertStore, SignatureScheme,
};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug)]
/// Custom rustls::ServerCertVerifier which captures certificates during verification
pub struct CapturingVerifier {
    /// A standard WebPkiServerVerifier used for verification
    inner: Arc<WebPkiServerVerifier>,
    /// A list of captured certificates in DER format
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

        if let Ok(mut lock) = self.captured_chain.try_lock() {
            *lock = Some(chain);
        } else {
            warn!("CaputuringVerifier.verify_server_cert try_lock failed")
        }

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

/// Builds a custom rustls ClientConfig using the CapturingVerifier
/// To be used with Reqwest::Client
pub fn capturing_verifier_tls_config(
    captured_chain: Arc<Mutex<Option<Vec<Vec<u8>>>>>,
) -> Result<ClientConfig, Box<dyn std::error::Error>> {
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

    Ok(config)
}

/// Converts DER encoded certificate bytes to PEM encoded certificate string
pub fn der_to_pem(der: &[u8]) -> String {
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
