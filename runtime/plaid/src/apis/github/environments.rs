use std::{collections::HashMap, sync::Arc};

use serde::Serialize;

use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use super::Github;

/// Policy that determines which branches can deploy.
/// Exactly one of the fields must be set to `true`.
#[derive(Serialize)]
struct DeploymentBranchPolicy {
    /// Only branches with branch protection rules can deploy.
    protected_branches: bool,
    /// Only branches that match the specified name patterns can deploy.
    custom_branch_policies: bool,
}

/// Represents someone who can review a deployment job
#[derive(Serialize)]
struct Reviewer {
    /// The type of reviewer (user or team)
    #[serde(rename = "type")]
    type_: String,
    /// The ID of the reviewer
    id: u64,
}

/// Payload sent to the GitHub REST API to create a new deployment environment.
/// See https://docs.github.com/en/rest/deployments/environments?apiVersion=2022-11-28#create-or-update-an-environment for details
#[derive(Serialize)]
struct CreateEnvironmentPayload {
    /// The amount of time (in minutes) to delay a job after the job is initially triggered
    wait_timer: u16,
    /// Whether or not a user who created the job is prevented from approving their own job
    prevent_self_review: bool,
    /// The people or teams that may review jobs that reference the environment
    reviewers: Vec<Reviewer>,
    /// The type of deployment branch policy for this environment
    deployment_branch_policy: DeploymentBranchPolicy,
}

/// Payload sent to the GitHub REST API to create a new deployment branch policy.
#[derive(Serialize)]
struct CreateDeploymentBranchPolicyPayload {
    /// The name pattern that branches or tags must match in order to deploy to the environment.
    name: String,
    #[serde(rename = "type")]
    /// Whether this rule targets a branch or tag.
    type_: String,
}

impl Github {
    /// Create a new GitHub deployment environment for a given repository
    /// See https://docs.github.com/en/rest/deployments/environments?apiVersion=2022-11-28#create-or-update-an-environment for more detail
    pub async fn create_environment_for_repo(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let env_name =
            self.validate_environment_name(request.get("env_name").ok_or(ApiError::BadRequest)?)?;

        info!(
            "Creating and configuring environment [{env_name}] in repo [{owner}/{repo}] on behalf of [{module}]"
        );

        let address = format!("/repos/{owner}/{repo}/environments/{env_name}");

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

    /// Configure a deployment branch protection rule for a GitHub deployment environment
    /// See https://docs.github.com/en/rest/deployments/branch-policies?apiVersion=2022-11-28#create-a-deployment-branch-policy for more details
    pub async fn create_deployment_branch_protection_rule(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(request.get("owner").ok_or(ApiError::BadRequest)?)?;
        let repo =
            self.validate_repository_name(request.get("repo").ok_or(ApiError::BadRequest)?)?;
        let env_name =
            self.validate_environment_name(request.get("env_name").ok_or(ApiError::BadRequest)?)?;
        let branch: &str =
            self.validate_branch_name(request.get("branch").ok_or(ApiError::BadRequest)?)?;

        info!(
            "Creating deployment branch protection rule for branch [{branch}] and environment [{env_name}] in repo [{owner}/{repo}] on behalf of [{module}]"
        );

        let address =
            format!("/repos/{owner}/{repo}/environments/{env_name}/deployment-branch-policies");

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
}
