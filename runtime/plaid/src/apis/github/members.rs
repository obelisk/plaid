use plaid_stl::github::GitHubUser;

use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};
use std::{collections::HashMap, sync::Arc};

impl Github {
    /// Check if a user belongs to an org
    /// See https://docs.github.com/en/rest/orgs/members?apiVersion=2022-11-28#check-organization-membership-for-a-user
    pub async fn check_org_membership_of_user(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<bool, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Parse out all the parameters from our parameter string
        let org = self.validate_org(request.get("org").ok_or(ApiError::BadRequest)?)?;
        let user = self.validate_username(request.get("user").ok_or(ApiError::BadRequest)?)?;

        info!("Checking if user [{user}] is part the [{org}] org on behalf of {module}");
        let address = format!("/orgs/{org}/members/{user}");

        match self.make_generic_get_request(address, module).await {
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

    /// Get a user's ID from their username
    /// See https://docs.github.com/en/rest/users/users?apiVersion=2022-11-28#get-a-user
    pub async fn get_user_id_from_username(
        &self,
        username: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let username = self.validate_username(username)?;

        info!("Getting user ID for username [{username}] on behalf of [{module}]");
        let address = format!("/users/{username}");

        match self.make_generic_get_request(address, module).await {
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
        user_id: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let user_id = self.validate_user_id(user_id)?;

        info!("Getting username for user ID [{user_id}] on behalf of [{module}]");
        let address = format!("/user/{user_id}");

        match self.make_generic_get_request(address, module).await {
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
