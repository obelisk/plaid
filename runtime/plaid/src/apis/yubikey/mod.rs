mod otp;

use reqwest::Client;

use serde::{Deserialize, Deserializer};

use std::time::Duration;

use ring::{
    hmac::{self, Key},
    rand::SystemRandom,
};

use super::default_timeout_seconds;

/// Validate that the client_id is not zero
fn validate_client_id<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u64::deserialize(deserializer)?;
    if value == 0 {
        return Err(serde::de::Error::custom("Yubico client_id must not be 0"));
    }
    Ok(value)
}

#[derive(Deserialize)]
pub struct YubikeyConfig {
    /// Client ID for the Yubico API service
    #[serde(deserialize_with = "validate_client_id")]
    client_id: u64,
    /// Secret key for the Yubico API service
    secret_key: String,
    /// The number of seconds until an external API request times out.
    /// If no value is provided, the result of `default_timeout_seconds()` will be used.
    #[serde(default = "default_timeout_seconds")]
    api_timeout_seconds: u64,
}

/// The YubiKey API
pub struct Yubikey {
    /// Config for the YubiKey API
    config: YubikeyConfig,
    /// A client to make requests with
    client: Client,
    /// A key used for HMAC signing
    key: Key,
    /// A secure source of random values
    rng: SystemRandom,
}

#[derive(Debug)]
pub enum YubikeyError {
    RandError,
    NetworkError,
    NoStatus,
    NoData,
    BadData,
    NoSignature,
    BadSignature,
    NoSuchClient,
    OperationNotAllowed,
    MissingParameter,
    NotEnoughAnswers,
    NonceMismatch,
    SignatureMismatch,
    Other(String),
}

impl Yubikey {
    pub fn new(config: YubikeyConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_seconds))
            .build()
            .unwrap();

        let key = Key::new(
            hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
            &base64::decode(&config.secret_key).unwrap(),
        );
        let rng = SystemRandom::new();

        Self {
            config,
            client,
            key,
            rng,
        }
    }
}
