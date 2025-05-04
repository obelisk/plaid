use plaid_stl::github::PullRequestRequestReviewers;
use serde::Serialize;

use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};
use std::sync::Arc;

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
}
