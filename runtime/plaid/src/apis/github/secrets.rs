use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use alkali::asymmetric::seal;
use plaid_stl::github::{
    AddOrRemoveRepoToOrgSecretParams, ConfigureSecretParams, GithubApiWrapper,
    ListOrgSecretsForRepoParams,
};
use serde::Serialize;
use serde_json::Value;
use std::{fmt::Display, sync::Arc};

/// GitHub's maximum secret size limit in bytes (48KiB)
const GITHUB_SECRET_MAX_BYTES: usize = 48 * 1024; // 49,152 bytes

#[derive(Serialize)]
struct UploadEnvironmentSecretPayload {
    encrypted_value: String,
    key_id: String,
}

enum RepoToOrgSecretAction {
    Add,
    Remove,
}

impl Display for RepoToOrgSecretAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepoToOrgSecretAction::Add => write!(f, "added"),
            RepoToOrgSecretAction::Remove => write!(f, "removed"),
        }
    }
}

impl Github {
    /// Fetch the public key and key id from GitHub for encrypting a secret,
    /// given the API address to fetch the public key from (which differs based on the type of secret)
    async fn get_pub_key_for_secret_encryption(
        &self,
        client_id: impl Display,
        address: impl Display,
        module: Arc<PlaidModule>,
    ) -> Result<(String, String), ApiError> {
        let (status, body) = self
            .make_generic_get_request(client_id, address.to_string(), module.clone())
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

        Ok((pub_key.to_string(), key_id.to_string()))
    }

    /// Configure a secret in a GitHub repository or deployment environment
    pub async fn configure_secret(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<ConfigureSecretParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.params.owner)?;
        let repo = self.validate_repository_name(&request.params.repo)?;
        // Validate an env name if present
        let env_name = match &request.params.env_name {
            Some(name) => Some(self.validate_environment_name(name)?),
            None => None,
        };
        let secret_name = self.validate_secret_name(&request.params.secret_name)?;
        let secret = &request.params.secret;

        // Validate secret length against GitHub's 48KiB limit
        if secret.len() > GITHUB_SECRET_MAX_BYTES {
            return Err(ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Secret exceeds GitHub's 48KiB limit: {} bytes",
                secret.len()
            ))));
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
        let (pub_key, key_id) = self
            .get_pub_key_for_secret_encryption(&request.client_id, address, module.clone())
            .await?;

        // 2. Encrypt the secret under the pub key

        let mut ciphertext = vec![0u8; secret.as_bytes().len() + seal::OVERHEAD_LENGTH];
        seal::encrypt(
            secret.as_bytes(),
            base64::decode(pub_key)
                .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))?
                .as_slice()
                .try_into()
                .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))?,
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
            .make_generic_put_request(&request.client_id, address, Some(&body), module)
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

    async fn execute_org_secret_action(
        &self,
        client_id: impl Display,
        request: &AddOrRemoveRepoToOrgSecretParams,
        action: RepoToOrgSecretAction,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let org = self.validate_org(&request.org)?;
        let secret_name = self.validate_secret_name(&request.secret_name)?;
        let repository = self.validate_repository_name(&request.repository)?;

        let repo_id = self
            .get_repo_id_from_repo_name_internal(
                &client_id.to_string(),
                &org,
                &repository,
                module.clone(),
            )
            .await?;

        let address = format!("/orgs/{org}/actions/secrets/{secret_name}/repositories/{repo_id}");

        // Adding or removing a repo from an org secret uses the same endpoint, just with a different
        // HTTP verb, so we can unify the logic for both actions. Depending on the action, we will call
        // a different function and log a different message, but the rest of the logic is the same.
        // So we use a macro to avoid duplicating code and to keep the match DRY.
        macro_rules! org_secret_action {
            ($method:ident, $action:expr) => {
                match self.$method(client_id, address, None::<&String>, module.clone()).await {
                    Ok((status, _)) if status == 204 => {
                        info!("[{module}] {action} organization secret [{secret_name}] on [{repository}]");
                        Ok(0)
                    }
                    Ok((status, _)) => Err(ApiError::GitHubError(
                        GitHubError::UnexpectedStatusCode(status),
                    )),
                    Err(e) => Err(e),
                }
            };
        }

        match action {
            RepoToOrgSecretAction::Add => org_secret_action!(make_generic_put_request, action),
            RepoToOrgSecretAction::Remove => {
                org_secret_action!(make_generic_delete_request, action)
            }
        }
    }

    /// Add a repository to the list of repositories that have access to an organization secret
    /// https://docs.github.com/en/rest/actions/secrets?apiVersion=2026-03-10#add-selected-repository-to-an-organization-secret
    pub async fn add_repo_to_org_secret(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<AddOrRemoveRepoToOrgSecretParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        self.execute_org_secret_action(
            request.client_id,
            &request.params,
            RepoToOrgSecretAction::Add,
            module,
        )
        .await
    }

    /// Remove a repository from the list of repositories that have access to an organization secret
    /// https://docs.github.com/en/rest/actions/secrets?apiVersion=2026-03-10#remove-selected-repository-from-an-organization-secret
    pub async fn remove_repo_from_org_secret(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<AddOrRemoveRepoToOrgSecretParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        self.execute_org_secret_action(
            request.client_id,
            &request.params,
            RepoToOrgSecretAction::Remove,
            module,
        )
        .await
    }

    /// List the organization secrets that a repository has access to
    /// https://docs.github.com/en/rest/actions/secrets?apiVersion=2026-03-10#list-repository-organization-secrets
    pub async fn list_org_secrets_for_repo(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<ListOrgSecretsForRepoParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let org = self.validate_org(&request.params.org)?;
        let repository = self.validate_repository_name(&request.params.repository)?;

        // Note: we do not validate these values because they come in encoded as u32, and that's all we need.
        // Therefore, the validation is in the fact that they were able to be deserialized.
        let per_page = request.params.per_page.unwrap_or(30);
        let page = request.params.page.unwrap_or(1);

        info!("Listing organization secrets that can be accessed by repository [{repository}] on behalf of [{module}]");

        let url = format!("/repos/{org}/{repository}/actions/organization-secrets?per_page={per_page}&page={page}");

        match self
            .make_generic_get_request(request.client_id, url, module)
            .await
        {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }
}
