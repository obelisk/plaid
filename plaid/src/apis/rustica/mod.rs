use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, KeyPair};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use time::{Duration, OffsetDateTime};

use super::ApiError;

#[derive(Deserialize)]
pub struct Environment {
    pub tls_ca: String,
    pub mtls_cn: String,
    pub mtls_key: String,
    pub server: String,
}

#[derive(Serialize)]
pub struct ServerConfiguration {
    pub address: String,
    pub ca_pem: String,
    pub mtls_cert: String,
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
    pub async fn new_mtls_cert(&self, params: &str, _: &str) -> Result<String, ApiError> {
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

        let ca_key = KeyPair::from_pem(&&environment.mtls_key).map_err(|e| {
            error!("Rustica Error: {:?}", e);
            ApiError::ConfigurationError(format!(
                "Rustica environment [{environment_name}] has badly formatted key"
            ))
        })?;

        let alg = ca_key
            .compatible_algs()
            .next()
            .ok_or(ApiError::ConfigurationError(format!(
                "Rustica environment [{environment_name}] has an certificate with no discernible signature algorithm"
            )))?;

        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, &environment.mtls_cn);

        let mut mtls_ca_params = CertificateParams::default();
        mtls_ca_params.key_pair = Some(ca_key);
        mtls_ca_params.alg = alg;
        mtls_ca_params.distinguished_name = dn;

        let ca = Certificate::from_params(mtls_ca_params).map_err(|_| {
            ApiError::ConfigurationError(format!(
                "Rustica environment [{environment_name}] has bad certificate and private key pair"
            ))
        })?;

        let mut certificate_params = CertificateParams::new(vec![user_identity.to_string()]);
        certificate_params
            .distinguished_name
            .push(DnType::CommonName, user_identity.to_string());

        // TODO: This should probably be the way to do it, but Rustica seems
        // to want it in SAN DNS
        //certificate_params.subject_alt_names = vec![SanType::Rfc822Name(user_identity.to_string())];
        certificate_params.not_before = OffsetDateTime::now_utc();
        certificate_params.not_after = certificate_params
            .not_before
            .saturating_add(Duration::seconds(60 * 60 * 24 * 90));

        let new_certificate = Certificate::from_params(certificate_params).map_err(|_| {
            ApiError::RusticaError(RusticaError::UnserializableCertificate(
                user_identity.to_string(),
            ))
        })?;

        let user_private_key = new_certificate.serialize_private_key_pem();

        let user_certificate = new_certificate
            .serialize_pem_with_signer(&ca)
            .map_err(|_| {
                ApiError::RusticaError(RusticaError::UnserializableCertificate(
                    user_identity.to_string(),
                ))
            })?;

        let server_config = ServerConfiguration {
            address: environment.server.clone(),
            ca_pem: environment.tls_ca.clone(),
            mtls_cert: user_certificate,
            mtls_key: user_private_key,
        };

        Ok(serde_json::to_string(&server_config).map_err(|_| {
            ApiError::RusticaError(RusticaError::UnserializableCertificate(
                user_identity.to_string(),
            ))
        })?)
    }
}
