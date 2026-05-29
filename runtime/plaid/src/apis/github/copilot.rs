use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};
use plaid_stl::github::{
    AddUsersToOrgCopilotParams, GithubApiWrapper, ListSeatsInOrgCopilotParams,
    RemoveUsersFromOrgCopilotParams,
};

use std::sync::Arc;

impl Github {
    /// List all seats in the Copilot subscription for an organization
    /// See https://docs.github.com/en/rest/copilot/copilot-user-management?apiVersion=2022-11-28#list-all-copilot-seat-assignments-for-an-organization for more details
    pub async fn list_seats_in_org_copilot(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<ListSeatsInOrgCopilotParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let organization = self.validate_org(&request.params.org)?;
        let per_page: u8 = request.params.per_page.unwrap_or(50);
        let page: u16 = request.params.page.unwrap_or(1);

        info!("List seats in Copilot subscription for org {organization}");

        let address =
            format!("/orgs/{organization}/copilot/billing/seats?page={page}&per_page={per_page}");

        match self
            .make_generic_get_request(&request.client_id, address, module)
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

    /// Add users to the Copilot subscription for an organization
    /// See https://docs.github.com/en/rest/copilot/copilot-user-management?apiVersion=2022-11-28#add-users-to-the-copilot-subscription-for-an-organization for more details
    pub async fn add_users_to_org_copilot(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<AddUsersToOrgCopilotParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let organization = self.validate_org(&request.params.org)?;
        for username in &request.params.selected_usernames {
            self.validate_username(&username)?;
        }

        info!(
            "Adding users {:?} to Copilot subscription for org {organization} as module {module}",
            request.params.selected_usernames
        );

        let address = format!("/orgs/{organization}/copilot/billing/selected_users");

        match self
            .make_generic_post_request(&request.client_id, address, &request, module)
            .await
        {
            Ok((status, Ok(body))) => {
                if status == 201 {
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

    /// Remove users from the Copilot subscription for an organization
    /// See https://docs.github.com/en/rest/copilot/copilot-user-management?apiVersion=2022-11-28#remove-users-from-the-copilot-subscription-for-an-organization for more details
    pub async fn remove_users_from_org_copilot(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<RemoveUsersFromOrgCopilotParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let organization = self.validate_org(&request.params.org)?;
        for username in request.params.selected_usernames.iter() {
            self.validate_username(&username)?;
        }

        info!(
            "Remove users {:?} from Copilot subscription for org {organization} as module {module}",
            request.params.selected_usernames
        );

        let address = format!("/orgs/{organization}/copilot/billing/selected_users");

        match self
            .make_generic_delete_request(&request.client_id, address, Some(&request), module)
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
