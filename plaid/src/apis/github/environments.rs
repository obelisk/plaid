use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use alkali::asymmetric::seal;

use crate::apis::{github::GitHubError, ApiError};

use super::Github;

#[derive(Serialize)]
struct DeploymentBranchPolicy {
    protected_branches: bool,
    custom_branch_policies: bool,
}

#[derive(Serialize)]
struct CreateEnvironmentPayload {
    wait_timer: u16,
    prevent_self_review: bool,
    reviewers: Vec<String>, // This is not true (items are not strings) but we will leave it empty, so we don't care
    deployment_branch_policy: DeploymentBranchPolicy,
}

#[derive(Serialize)]
struct CreateDeploymentBranchPolicyPayload {
    name: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Serialize)]
struct UploadEnvironmentSecretPayload {
    encrypted_value: String,
    key_id: String,
}

impl Github {
    /// Create a new GitHub deployment environment for a given repository
    /// See https://docs.github.com/en/rest/deployments/environments?apiVersion=2022-11-28#create-or-update-an-environment for more detail
    pub async fn create_environment_for_repo(
        &self,
        params: &str,
        module: &str,
    ) -> Result<u32, ApiError> {
        const ENVIRONMENT_NAME: &str = "publish";

        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let branch =
            self.validate_branch_name(request.get("branch").ok_or(ApiError::BadRequest)?)?;

        info!(
            "Creating and configuring 'publish' environment in repo [{repo}] owned by [{owner}] on behalf of [{module}]"
        );

        // 1. Create the environment

        let address = format!("/repos/{owner}/{repo}/environments/{ENVIRONMENT_NAME}");

        let body = CreateEnvironmentPayload {
            wait_timer: 0,
            prevent_self_review: false,
            reviewers: vec![],
            deployment_branch_policy: DeploymentBranchPolicy {
                protected_branches: false,
                custom_branch_policies: true,
            },
        };

        match self
            .make_generic_put_request(address, Some(&body), module)
            .await
        {
            Ok((status, _)) => {
                if status == 200 {
                    Ok(())
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Err(e) => Err(e),
        }?;

        // 2. Create custom deployment protection rule to allow deployments only from the given branch
        // See https://docs.github.com/en/rest/deployments/branch-policies?apiVersion=2022-11-28#create-a-deployment-branch-policy for more details

        let address = format!(
            "/repos/{owner}/{repo}/environments/{ENVIRONMENT_NAME}/deployment-branch-policies"
        );

        let body = CreateDeploymentBranchPolicyPayload {
            name: branch.to_string(),
            type_: "branch".to_string(), // it must be a branch, meaning it cannot be a tag that matches the given name
        };

        match self
            .make_generic_post_request(address, Some(&body), module)
            .await
        {
            Ok((status, _)) => {
                if status == 200 {
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

    /// Configure a secret in a GitHub deployment environment
    pub async fn configure_environment_secret(
        &self,
        params: &str,
        module: &str,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let environment_name = request.get("env_name").ok_or(ApiError::BadRequest)?;
        let secret_name = request.get("secret_name").ok_or(ApiError::BadRequest)?;
        let secret = request.get("secret").ok_or(ApiError::BadRequest)?;

        info!("Configuring secret with name [{secret_name}] on environment [{environment_name}] for repository [{owner}/{repo}] on behalf of [{module}]");

        // 1. Get pub key for the environment
        let address =
            format!("/repos/{owner}/{repo}/environments/{environment_name}/secrets/public-key");
        let (status, body) = self.make_generic_get_request(address, module).await?;
        if status != 200 {
            return Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                status,
            )));
        };
        let res = serde_json::from_str::<Value>(&body?).map_err(|_| {
            ApiError::GitHubError(GitHubError::InvalidInput(
                "Could not deserialize environment's public key".to_string(),
            ))
        })?;
        let env_pub_key = res
            .get("key")
            .ok_or(ApiError::GitHubError(GitHubError::InvalidInput(
                "Invalid response while fetching environment's public key".to_string(),
            )))?
            .to_string()
            .replace("\"", "");
        let env_key_id = res
            .get("key_id")
            .ok_or(ApiError::GitHubError(GitHubError::InvalidInput(
                "Invalid response while fetching environment's public key".to_string(),
            )))?
            .to_string()
            .replace("\"", "");

        // 2. Encrypt the secret under the environment's pub key
        let mut ciphertext = vec![0u8; secret.as_bytes().len() + seal::OVERHEAD_LENGTH];
        seal::encrypt(
            secret.as_bytes(),
            base64::decode(env_pub_key)
                .unwrap()
                .as_slice()
                .try_into()
                .unwrap(),
            &mut ciphertext,
        )
        .map_err(|_| {
            ApiError::GitHubError(GitHubError::InvalidInput(
                "Could not encrypt secret under environment's public key".to_string(),
            ))
        })?;
        let ciphertext = base64::encode(ciphertext);

        // 3. Upload the encrypted secret via the REST API
        let address =
            format!("/repos/{owner}/{repo}/environments/{environment_name}/secrets/{secret_name}");

        let body = UploadEnvironmentSecretPayload {
            encrypted_value: ciphertext,
            key_id: env_key_id,
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
