use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use x509_parser::prelude::*;

/// A permissive verifier that accepts all certificates.
/// We use this because we are not sure the SSL certs we are checking are all valid.
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

/// Connects via TLS and retrieves the peer certificates.
/// - `destination_domain`: domain of the TCP endpoint
/// - `sni`: SNI hostname (e.g., Cloudflare proxy domain)
pub async fn get_peer_certificate_with_sni(
    destination_domain: &str,
    sni: &str,
) -> Result<String, String> {
    // Build a configuration which has no root certificates and does not verify anything.
    // We isolate this code in this function to limit the scope of the dangerous configuration.
    let mut cfg = ClientConfig::builder()
        .with_root_certificates(RootCertStore::empty())
        .with_no_client_auth();
    cfg.dangerous()
        .set_certificate_verifier(Arc::new(NoVerifier));

    let connector = TlsConnector::from(Arc::new(cfg));

    // Connect TCP. We hardcode port 443 for TLS: in theory other ports could be used, but
    // this is the vast majority of use cases and we don't see a need for more flexibility now.
    let tcp = TcpStream::connect(format!("{destination_domain}:443"))
        .await
        .map_err(|_| "Failed to connect TCP".to_string())?;

    let server_name = ServerName::try_from(sni)
        .map_err(|_| "Invalid SNI".to_string())?
        .to_owned();

    // Perform TLS handshake
    let tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(|_| "TLS handshake failed".to_string())?;
    let (_io, conn) = tls.get_ref();
    let certs = conn.peer_certificates().unwrap_or_default();

    // Even if there are multiple certificates, we are interested in the first one (the leaf)

    let leaf = certs
        .iter()
        .next()
        .ok_or("No certificates found".to_string())?;
    let (_, cert) = X509Certificate::from_der(leaf.as_ref())
        .map_err(|_e| "Failed to parse certificate".to_string())?;

    let encoded = base64::encode(cert.as_raw());
    Ok(encoded)
}
