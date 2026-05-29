use std::fmt::Display;

use crate::{github::GithubApiWrapper, PlaidFunctionError};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CheckOrgMembershipParams {
    pub user: String,
    pub org: String,
}

/// Check whether a user belongs to an org
/// ## Arguments
///
/// * `user` - The account to check org membership of
/// * `org` - The org to check if `user` is part of
pub fn check_org_membership_of_user(
    client_id: impl Display,
    user: impl Display,
    org: impl Display,
) -> Result<bool, PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, check_org_membership_of_user);
    }
    let params = CheckOrgMembershipParams {
        user: user.to_string(),
        org: org.to_string(),
    };
    let wrapped = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };
    let request = serde_json::to_string(&wrapped).unwrap();

    let res = unsafe {
        github_check_org_membership_of_user(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    Ok(res != 0)
}
