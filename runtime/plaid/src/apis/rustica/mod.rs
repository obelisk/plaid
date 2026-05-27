use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use serde::{Deserialize, Serialize};

use std::{collections::HashMap, sync::Arc};
use time::{Duration, OffsetDateTime};

use crate::loader::PlaidModule;

use super::ApiError;

#[derive(Deserialize)]
pub struct Environment {
    /// Public key for the CA
    pub tls_ca: String,
    /// Common name for the certificate
    pub mtls_cn: String,
    /// Key pair for the CA
    pub mtls_key: String,
    /// Server address
    pub server: String,
}

#[derive(Serialize)]
pub struct ServerConfiguration {
    /// Server address
    pub address: String,
    /// Public key for the CA
    pub ca_pem: String,
    /// mTLS user certificate
    pub mtls_cert: String,
    /// mTLS user private key
    pub mtls_key: String,
}

#[derive(Deserialize)]
pub struct RusticaConfig {
    /// This contains the mapping of available Rustica environments
    /// to their associated CA private keys. These are used for
    /// rules to be able to create new mTLS certificates used for
    /// access these environments
    environments: HashMap<String, Environment>,
}

pub struct Rustica {
    config: RusticaConfig,
}

#[derive(Debug)]
pub enum RusticaError {
    UnserializableCertificate(String),
    UnknownEnvironment(String),
}

impl Rustica {
    pub fn new(config: RusticaConfig) -> Self {
        Self { config }
    }
}

impl Rustica {
    pub(crate) fn generate_server_configuration(
        environment_name: &str,
        user_identity: &str,
        environment: &Environment,
    ) -> Result<ServerConfiguration, ApiError> {
        let user_identity_owned = user_identity.to_string();

        let ca_key = KeyPair::from_pem(&environment.mtls_key).map_err(|e| {
            error!("Rustica Error: {:?}", e);
            ApiError::ConfigurationError(format!(
                "Rustica environment [{environment_name}] has an incorrectly formatted key"
            ))
        })?;

        let alg = ca_key.compatible_algs().next().ok_or(
            ApiError::ConfigurationError(format!(
                "Rustica environment [{environment_name}] has a certificate with no known signature algorithm"
            )),
        )?;

        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, &environment.mtls_cn);

        let mut mtls_ca_params = CertificateParams::default();
        mtls_ca_params.distinguished_name = dn;

        let ca = mtls_ca_params.self_signed(&ca_key).map_err(|_| {
            ApiError::ConfigurationError(format!(
                "Rustica environment [{environment_name}] has bad certificate and private key pair"
            ))
        })?;

        let mut certificate_params = CertificateParams::new(vec![user_identity_owned.clone()])
            .map_err(|_| {
                ApiError::RusticaError(RusticaError::UnserializableCertificate(
                    user_identity_owned.clone(),
                ))
            })?;
        certificate_params
            .distinguished_name
            .push(DnType::CommonName, user_identity_owned.clone());

        // TODO: This should probably be the way to do it, but Rustica seems
        // to want it in SAN DNS
        //certificate_params.subject_alt_names = vec![SanType::Rfc822Name(user_identity_owned.clone())];
        certificate_params.not_before = OffsetDateTime::now_utc();
        certificate_params.not_after = certificate_params
            .not_before
            .saturating_add(Duration::seconds(60 * 60 * 24 * 90));

        let user_key_pair = KeyPair::generate_for(alg).map_err(|_| {
            ApiError::RusticaError(RusticaError::UnserializableCertificate(
                user_identity_owned.clone(),
            ))
        })?;

        let new_certificate = certificate_params
            .signed_by(&user_key_pair, &ca, &ca_key)
            .map_err(|_| {
                ApiError::RusticaError(RusticaError::UnserializableCertificate(
                    user_identity_owned.clone(),
                ))
            })?;

        let user_private_key = user_key_pair.serialize_pem();
        let user_certificate = new_certificate.pem();

        Ok(ServerConfiguration {
            address: environment.server.clone(),
            ca_pem: environment.tls_ca.clone(),
            mtls_cert: user_certificate,
            mtls_key: user_private_key,
        })
    }

    /// Create a new mTLS certificate and return a serialized `ServerConfiguration` object
    pub async fn new_mtls_cert(
        &self,
        params: &str,
        _: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let environment_name = request
            .get("environment")
            .ok_or(ApiError::MissingParameter("environment".to_owned()))?;
        let user_identity = request
            .get("identity")
            .ok_or(ApiError::MissingParameter("identity".to_owned()))?;

        let environment =
            self.config
                .environments
                .get(*environment_name)
                .ok_or(ApiError::RusticaError(RusticaError::UnknownEnvironment(
                    environment_name.to_string(),
                )))?;

        let server_config =
            Self::generate_server_configuration(environment_name, user_identity, environment)?;

        Ok(serde_json::to_string(&server_config).map_err(|_| {
            ApiError::RusticaError(RusticaError::UnserializableCertificate(
                user_identity.to_string(),
            ))
        })?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{BasicConstraints, IsCa};
    use x509_parser::pem::parse_x509_pem;
    use x509_parser::prelude::*;

    /// Build a throwaway self-signed CA and return (ca_cert_pem, ca_key_pem).
    fn generate_test_ca(cn: &str) -> (String, String) {
        let ca_key = KeyPair::generate().expect("Test CA key generation failed");

        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, cn);

        let mut params = CertificateParams::default();
        params.distinguished_name = dn;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

        let cert = params
            .self_signed(&ca_key)
            .expect("Test CA self-signing failed");

        (cert.pem(), ca_key.serialize_pem())
    }

    #[test]
    fn test_generate_server_configuration_valid_certificate() {
        let user_identity = "mitchell@example.io";
        let server_address = "rustica.example.io:7001";
        let ca_cn = "Test mTLS CA";

        let (ca_cert_pem, ca_key_pem) = generate_test_ca(ca_cn);

        let environment = Environment {
            tls_ca: ca_cert_pem.clone(),
            mtls_cn: ca_cn.to_string(),
            mtls_key: ca_key_pem,
            server: server_address.to_string(),
        };

        let config =
            Rustica::generate_server_configuration("test-env", user_identity, &environment)
                .expect("generate_server_configuration failed");

        // Server address and TLS CA pass through unchanged.
        assert_eq!(config.address, server_address);
        assert_eq!(config.ca_pem, ca_cert_pem);

        // The private key must be a valid PEM-encoded key pair.
        assert!(
            KeyPair::from_pem(&config.mtls_key).is_ok(),
            "returned mtls_key is not a valid PEM key"
        );

        // Parse the issued certificate with x509-parser and verify its fields.
        let (_, pem) =
            parse_x509_pem(config.mtls_cert.as_bytes()).expect("mtls_cert is not valid PEM");
        let (_, cert) =
            parse_x509_certificate(&pem.contents).expect("mtls_cert is not a valid X.509 cert");

        // Subject CN must equal the user identity.
        let cn = cert
            .subject()
            .iter_common_name()
            .next()
            .expect("cert has no CN")
            .as_str()
            .expect("CN is not a UTF-8 string");
        assert_eq!(cn, user_identity);

        // The SAN DNS entry must also equal the user identity.
        let san = cert
            .subject_alternative_name()
            .expect("SAN extension parse error")
            .expect("cert has no SAN extension");
        let has_dns_san = san
            .value
            .general_names
            .iter()
            .any(|gn| matches!(gn, GeneralName::DNSName(name) if *name == user_identity));
        assert!(
            has_dns_san,
            "expected SAN DNS name {user_identity} not found"
        );

        // Validity window should be approximately 90 days
        let validity = cert.validity();
        let duration =
            (validity.not_after - validity.not_before).expect("validity period subtraction failed");
        let days = duration.whole_days();
        assert!(
            (89..=91).contains(&days),
            "expected ~90 day validity, got {days} days"
        );

        // Certificate must currently be valid.
        assert!(
            validity.is_valid(),
            "generated certificate is not currently valid"
        );
    }
}
