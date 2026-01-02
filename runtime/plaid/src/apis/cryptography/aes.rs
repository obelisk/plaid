use std::{collections::HashMap, fmt::Display, sync::Arc};

use plaid_stl::cryptography::{AesDecryptPayload, AesEncryptPayload};
use serde::Deserialize;

use crate::{
    apis::{cryptography::Cryptography, ApiError},
    cryptography,
    loader::PlaidModule,
};

/// Action performed with an AES key
#[derive(Deserialize, PartialEq, Clone)]
enum AesAction {
    Encrypt,
    Decrypt,
}

/// Specifications for a local AES key
#[derive(Deserialize, Clone)]
pub struct AesKeySpec {
    /// Identifier for a local AES key
    id: String,
    /// The key material, hex encoded
    key: String,
    /// Map between rule names and list of allowed actions
    rules_and_actions: HashMap<String, Vec<AesAction>>,
}

/// Configuration for using local AES keys
#[derive(Deserialize)]
pub struct AesConfig {
    key_specs: Vec<AesKeySpec>,
}

pub struct Aes {
    /// Map {key ID --> key spec}
    key_specs: HashMap<String, AesKeySpec>,
}

impl Aes {
    pub fn new(config: AesConfig) -> Self {
        let key_specs: HashMap<String, AesKeySpec> = config
            .key_specs
            .iter()
            .map(|ks| (ks.id.clone(), ks.clone()))
            .collect();
        Self { key_specs }
    }
}

impl Cryptography {
    /// Return the key if a module can perform a certain action on a given AES key, otherwise None
    fn get_aes_key_for_action(
        &self,
        module: impl Display,
        key_id: impl Display,
        action: AesAction,
    ) -> Option<String> {
        self.aes.as_ref().and_then(|aes| {
            aes.key_specs.get(&key_id.to_string()).and_then(|key_spec| {
                key_spec
                    .rules_and_actions
                    .get(&module.to_string())
                    .and_then(|allowed_actions| {
                        if allowed_actions.contains(&action) {
                            Some(key_spec.key.clone())
                        } else {
                            None
                        }
                    })
            })
        })
    }

    /// Perform an AES encryption using a key defined in Plaid's config.
    pub async fn aes_128_cbc_encrypt(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        if self.aes.is_none() {
            return Err(ApiError::CryptographyError(
                "API not configured".to_string(),
            ));
        }

        let payload: AesEncryptPayload = serde_json::from_str(&params)
            .map_err(|_| ApiError::CryptographyError("Failed to parse payload".to_string()))?;

        let key =
            match self.get_aes_key_for_action(&module.name, &payload.key_id, AesAction::Encrypt) {
                Some(key) => key,
                None => {
                    return Err(ApiError::CryptographyError(
                        "Missing key or operation not permitted".to_string(),
                    ))
                }
            };

        let key = hex::decode(key)
            .map_err(|_| ApiError::CryptographyError("Failed to decode key".to_string()))?;

        info!(
            "Performing an AES encryption with local key [{}] on behalf of module [{module}]",
            payload.key_id
        );

        cryptography::aes_128_cbc::encrypt(&key, &payload.plaintext.to_string())
            .map_err(|_| ApiError::CryptographyError("Failed to encrypt plaintext".to_string()))
    }

    /// Perform an AES decryption using a key defined in Plaid's config.
    pub async fn aes_128_cbc_decrypt(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        if self.aes.is_none() {
            return Err(ApiError::CryptographyError(
                "API not configured".to_string(),
            ));
        }

        let payload: AesDecryptPayload = serde_json::from_str(&params)
            .map_err(|_| ApiError::CryptographyError("Failed to parse payload".to_string()))?;

        let key =
            match self.get_aes_key_for_action(&module.name, &payload.key_id, AesAction::Decrypt) {
                Some(key) => key,
                None => {
                    return Err(ApiError::CryptographyError(
                        "Missing key or operation not permitted".to_string(),
                    ))
                }
            };

        let key = hex::decode(key)
            .map_err(|_| ApiError::CryptographyError("Failed to decode key".to_string()))?;

        info!(
            "Performing an AES decryption with local key [{}] on behalf of module [{module}]",
            payload.key_id
        );

        cryptography::aes_128_cbc::decrypt(&key, &payload.ciphertext.to_string())
            .map_err(|_| ApiError::CryptographyError("Failed to decrypt ciphertext".to_string()))
    }
}
