use plaid_stl::github::{
    CheckOrgMembershipParams, GitHubUser, GithubApiWrapper, RemoveOutsideCollaboratorParams,
};

use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};
use std::sync::Arc;

impl Github {
    /// Check if a user belongs to an org
    /// See https://docs.github.com/en/rest/orgs/members?apiVersion=2022-11-28#check-organization-membership-for-a-user
    pub async fn check_org_membership_of_user(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<bool, ApiError> {
        let request: GithubApiWrapper<CheckOrgMembershipParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters from our parameter string
        let org = self.validate_org(&request.params.org)?;
        let user = self.validate_username(&request.params.user)?;

        info!("Checking if user [{user}] is part the [{org}] org on behalf of {module}");
        let address = format!("/orgs/{org}/members/{user}");

        match self
            .make_generic_get_request(&request.client_id, address, module)
            .await
        {
            Ok((status, Ok(_))) => {
                if status == 204 {
                    Ok(true)
                } else if status == 404 {
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

    /// Remove an outside collaborator from an org.
    /// See https://docs.github.com/en/rest/orgs/outside-collaborators?apiVersion=2022-11-28#remove-outside-collaborator-from-an-organization
    pub async fn remove_outside_collaborator_from_org(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request: GithubApiWrapper<RemoveOutsideCollaboratorParams> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let org = self.validate_org(&request.params.org)?;
        let user = self.validate_username(&request.params.user)?;

        info!("Removing outside collaborator [{user}] from org [{org}] on behalf of {module}");
        let address = format!("/orgs/{org}/outside_collaborators/{user}");

        match self
            .make_generic_delete_request::<&str>(&request.client_id, address, None, module)
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

    /// Get a user's ID from their username
    /// See https://docs.github.com/en/rest/users/users?apiVersion=2022-11-28#get-a-user
    pub async fn get_user_id_from_username(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<String> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let username = self.validate_username(&request.params)?;

        info!("Getting user ID for username [{username}] on behalf of [{module}]");
        let address = format!("/users/{username}");

        match self
            .make_generic_get_request(&request.client_id, address, module)
            .await
        {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    let user_info: GitHubUser = serde_json::from_str(&body)
                        .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))?;
                    Ok(user_info.id.to_string())
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

    /// Get a user's username from their user ID
    /// See https://docs.github.com/en/rest/users/users?apiVersion=2022-11-28#get-a-user-using-their-id
    pub async fn get_username_from_user_id(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<String> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let user_id = self.validate_user_id(&request.params)?;

        info!("Getting username for user ID [{user_id}] on behalf of [{module}]");
        let address = format!("/user/{user_id}");

        match self
            .make_generic_get_request(&request.client_id, address, module)
            .await
        {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    let user_info: GitHubUser = serde_json::from_str(&body)
                        .map_err(|_| ApiError::GitHubError(GitHubError::BadResponse))?;
                    Ok(user_info.login)
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
