use plaid_stl::github::{
    CreatePullRequestRequest, GetPullRequestOptions, GetPullRequestRequest,
    PullRequestRequestReviewers,
};
use serde::Serialize;
use serde_json::json;

use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};
use std::sync::Arc;
use url::form_urlencoded::Serializer;

impl Github {
    /// Add reviewers to a pull request
    pub async fn pull_request_request_reviewers(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<bool, ApiError> {
        #[derive(Serialize)]
        struct RequestReviewers {
            reviewers: Vec<String>,
            team_reviewers: Vec<String>,
        }
        let request: PullRequestRequestReviewers =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters from our parameter string
        let owner = self.validate_org(&request.owner)?;
        let repo = self.validate_repository_name(&request.repo)?;
        let pull_number = request.pull_number;

        for reviewer in &request.reviewers {
            self.validate_username(&reviewer)?;
        }

        for team in &request.team_reviewers {
            self.validate_team_slug(&team)?;
        }

        info!("Requesting reviews from users: [{}] and teams: [{}] on [{owner}/{repo}/{pull_number}] org on behalf of {module}", request.reviewers.join(", "), request.team_reviewers.join(", "));

        let address = format!("/repos/{owner}/{repo}/pulls/{pull_number}/requested_reviewers");

        let body = RequestReviewers {
            reviewers: request.reviewers.clone(),
            team_reviewers: request.team_reviewers.clone(),
        };

        match self.make_generic_post_request(address, body, module).await {
            Ok((status, Ok(_))) => {
                if status == 201 {
                    Ok(true)
                } else if status == 404 {
                    Ok(false)
                } else if status == 422 {
                    warn!("Some of the reviewers or teams are not collaborators on this repository. Context: [{owner}/{repo}/{pull_number}]. Users: [{}] and teams: [{}]", request.reviewers.join(", "), request.team_reviewers.join(", "));
                    Ok(false)
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

    /// Creates a pull request in a specified repository.
    pub async fn create_pull_request(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: CreatePullRequestRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_org(&request.owner)?;
        let repo = self.validate_repository_name(&request.repo)?;

        // Build the request body, omitting optional fields if they are not set.
        let mut request_body = json!({
            "title": request.title,
            "head": request.head,
            "base": request.base,
            "draft": request.draft,
        });

        // Add the body if it exists
        if let Some(body) = request.body {
            request_body["body"] = json!(body);
        }

        let serialized = request_body.to_string();
        let address = format!("/repos/{owner}/{repo}/pulls");

        info!("Creating pull request in [{owner}/{repo}] org on behalf of {module}");

        match self
            .make_generic_post_request(address, serialized, module)
            .await
        {
            Ok((status, Ok(_))) => {
                if status == 201 {
                    Ok(0)
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

    /// Lists pull requests in a specified repository.
    pub async fn list_pull_requests(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GetPullRequestRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_org(&request.owner)?;
        let repo = self.validate_repository_name(&request.repo)?;

        if request.per_page > 100 {
            return Err(ApiError::BadRequest);
        }

        let options = request
            .options
            .map_or(Default::default(), query_string_from_options);

        info!("Listing pull requests in [{owner}/{repo}] org on behalf of {module}",);
        let address = format!("/repos/{owner}/{repo}/pulls?{options}");

        match self.make_generic_get_request(address, module).await {
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

/// Build a query string for the GitHub "List Pull Requests" API
/// (`GET /repos/{owner}/{repo}/pulls`) from the given options.
fn query_string_from_options(options: GetPullRequestOptions) -> String {
    let mut serializer = Serializer::new(String::new());
    if let Some(s) = options.state {
        serializer.append_pair("state", &s.to_string());
    }
    if let Some(h) = options.head.as_deref() {
        serializer.append_pair("head", h);
    }
    if let Some(b) = options.base.as_deref() {
        if !b.is_empty() {
            serializer.append_pair("base", b);
        }
    }
    serializer.finish()
}
