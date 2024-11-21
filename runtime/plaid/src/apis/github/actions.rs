use super::Github;
use crate::apis::{github::GitHubError, ApiError};
use plaid_stl::github::RepositoryDispatchParams;
use serde::Serialize;
use serde_json::Value;

/// Payload sent to the GH API when triggering a GH Actions workflow via repository dispatch
#[derive(Serialize)]
struct RepositoryDispatchPayload<'a, T>
where
    T: Serialize,
{
    event_type: &'a str,
    client_payload: &'a T,
}

impl Github {
    /// Trigger a GHA workflow via repository_dispatch
    /// For more details, see
    /// * https://docs.github.com/en/rest/repos/repos?apiVersion=2022-11-28#create-a-repository-dispatch-event
    /// * https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/events-that-trigger-workflows#repository_dispatch
    pub async fn trigger_repo_dispatch(&self, params: &str, module: &str) -> Result<u32, ApiError> {
        let request: RepositoryDispatchParams<Value> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let owner = self.validate_username(&request.owner)?;
        let repo = self.validate_repository_name(&request.repo)?;
        let event_type = self.validate_event_type(&request.event_type)?;
        // Since client_payload can be an arbitrary JSON, we "validate" it alredy when deserializing it to a Value.
        // However, we have no control over the content.

        info!("Triggering repository_dispatch GHA for repository [{owner}/{repo}] on behalf of [{module}]");

        let address = format!("/repos/{owner}/{repo}/dispatches");

        let body = RepositoryDispatchPayload {
            event_type,
            client_payload: &request.client_payload,
        };

        match self
            .make_generic_post_request(address, &body, &module)
            .await
        {
            Ok((status, Ok(_))) => {
                if status == 204 {
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
}
