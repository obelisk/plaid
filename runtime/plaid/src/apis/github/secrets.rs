use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use alkali::asymmetric::seal;
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

#[derive(Serialize)]
struct UploadEnvironmentSecretPayload {
    encrypted_value: String,
    key_id: String,
}

impl Github {
    /// Configure a secret in a GitHub repository or deployment environment
    pub async fn configure_secret(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        // Validate an env name if present
        let env_name = match request.get("env_name") {
            Some(name) => Some(self.validate_environment_name(name)?),
            None => None,
        };
        let secret_name =
            self.validate_secret_name(request.get("secret_name").ok_or(ApiError::BadRequest)?)?;
        let secret = request.get("secret").ok_or(ApiError::BadRequest)?;

        // Validate secret length against GitHub's 48KiB limit
        const GITHUB_SECRET_MAX_BYTES: usize = 48 * 1024; // 49,152 bytes
        if secret.len() > GITHUB_SECRET_MAX_BYTES {
            return Err(ApiError::GitHubError(GitHubError::InvalidInput(
                format!("Secret exceeds GitHub's 48KiB limit: {} bytes", secret.len()),
            )));
        }

        match env_name {
            Some(name) => info!("Configuring secret with name [{secret_name}] on environment [{name}] for repository [{owner}/{repo}] on behalf of [{module}]"),
            None => info!("Configuring secret with name [{secret_name}] on repository [{owner}/{repo}] on behalf of [{module}]"),
        }

        // 1. Get pub key

        let address = match env_name {
            Some(env_name) => {
                // We are setting an environment secret
                format!("/repos/{owner}/{repo}/environments/{env_name}/secrets/public-key")
            }
            None => {
                // We are setting a repository secret
                format!("/repos/{owner}/{repo}/actions/secrets/public-key")
            }
        };
        let (status, body) = self
            .make_generic_get_request(address, module.clone())
            .await?;
        if status != 200 {
            return Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                status,
            )));
        };
        let res = serde_json::from_str::<Value>(&body?).map_err(|_| {
            ApiError::GitHubError(GitHubError::InvalidInput(
                "Could not deserialize public key from GitHub".to_string(),
            ))
        })?;
        let pub_key = res
            .get("key")
            .ok_or(ApiError::GitHubError(GitHubError::InvalidInput(
                "Invalid response while fetching public key from GitHub".to_string(),
            )))?
            .as_str()
            .ok_or(ApiError::GitHubError(GitHubError::BadResponse))?;
        let key_id = res
            .get("key_id")
            .ok_or(ApiError::GitHubError(GitHubError::InvalidInput(
                "Invalid response while fetching public key from GitHub".to_string(),
            )))?
            .as_str()
            .ok_or(ApiError::GitHubError(GitHubError::BadResponse))?;

        // 2. Encrypt the secret under the pub key

        let mut ciphertext = vec![0u8; secret.as_bytes().len() + seal::OVERHEAD_LENGTH];
        seal::encrypt(
            secret.as_bytes(),
            base64::decode(pub_key)
                .unwrap()
                .as_slice()
                .try_into()
                .unwrap(),
            &mut ciphertext,
        )
        .map_err(|_| {
            ApiError::GitHubError(GitHubError::InvalidInput(
                "Could not encrypt secret under public key".to_string(),
            ))
        })?;
        let ciphertext = base64::encode(ciphertext);

        // 3. Upload the encrypted secret via the REST API

        let address = match env_name {
            Some(env_name) => {
                // We are setting an environment secret
                format!("/repos/{owner}/{repo}/environments/{env_name}/secrets/{secret_name}")
            }
            None => {
                // We are setting a repository secret
                format!("/repos/{owner}/{repo}/actions/secrets/{secret_name}")
            }
        };
        let body = UploadEnvironmentSecretPayload {
            encrypted_value: ciphertext,
            key_id: key_id.to_string(),
        };

        match self
            .make_generic_put_request(address, Some(&body), module)
            .await
        {
            Ok((status, _)) => {
                // we are OK with creating or updating
                if status == 201 || status == 204 {
                    info!("Secret successfully uploaded");
                    Ok(0)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Err(e) => Err(e),
        }
    }
}
