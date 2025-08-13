use std::{collections::HashMap, fmt::Display, sync::Arc};

use plaid_stl::aes::{AesDecryptPayload, AesEncryptPayload};
use serde::Deserialize;

use crate::{apis::ApiError, loader::PlaidModule};

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

    /// Return whether a module can perform a certain action on a given AES key
    fn can_module_perform_action(
        &self,
        module: impl Display,
        key_id: impl Display,
        action: AesAction,
    ) -> bool {
        self.key_specs
            .get(&key_id.to_string())
            .and_then(|key_spec| key_spec.rules_and_actions.get(&module.to_string()))
            .and_then(|allowed_actions| Some(allowed_actions.contains(&action)))
            == Some(true)
    }

    /// Perform an AES encryption using a key defined in Plaid's config.
    pub async fn encrypt_local(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let payload: AesEncryptPayload = serde_json::from_str(&params)
            .map_err(|_| ApiError::AesError("Failed to parse payload".to_string()))?;
        if self.can_module_perform_action(&module.name, &payload.key_id, AesAction::Encrypt) {
            info!(
                "Performing an AES encryption with local key [{}] on behalf of module [{module}]",
                payload.key_id
            );
            let key = self
                .key_specs
                .get(&payload.key_id.to_string())
                .unwrap() // OK because we checked above
                .key
                .clone();
            let key = hex::decode(key)
                .map_err(|_| ApiError::AesError("Failed to decode key".to_string()))?;
            blocksmith::encrypt(&key, &payload.plaintext.to_string())
                .map_err(|_| ApiError::AesError("Failed to encrypt plaintext".to_string()))
        } else {
            Err(ApiError::AesError(
                "Missing key or operation not permitted".to_string(),
            ))
        }
    }

    /// Perform an AES decryption using a key defined in Plaid's config.
    pub async fn decrypt_local(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let payload: AesDecryptPayload = serde_json::from_str(&params)
            .map_err(|_| ApiError::AesError("Failed to parse payload".to_string()))?;
        if self.can_module_perform_action(&module.name, &payload.key_id, AesAction::Decrypt) {
            info!(
                "Performing an AES decryption with local key [{}] on behalf of module [{module}]",
                payload.key_id
            );
            let key = self
                .key_specs
                .get(&payload.key_id.to_string())
                .unwrap() // OK because we checked above
                .key
                .clone();
            let key = hex::decode(key)
                .map_err(|_| ApiError::AesError("Failed to decode key".to_string()))?;
            blocksmith::decrypt(&key, &payload.ciphertext.to_string())
                .map_err(|_| ApiError::AesError("Failed to decrypt ciphertext".to_string()))
        } else {
            Err(ApiError::AesError(
                "Missing key or operation not permitted".to_string(),
            ))
        }
    }
}
