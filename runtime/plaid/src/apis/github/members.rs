use crate::apis::{ApiError, github::GitHubError};
use std::collections::HashMap;
use super::Github;

impl Github {
    /// Check if a user belongs to an org
    /// See https://docs.github.com/en/rest/orgs/members?apiVersion=2022-11-28#check-organization-membership-for-a-user
    /// We return u8 instead of bool here because impl_new_function_with_error_buffer does not
    /// support bool
    pub async fn check_org_membership_of_user(&self, params: &str, module: &str) -> Result<bool, ApiError> {
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
}
