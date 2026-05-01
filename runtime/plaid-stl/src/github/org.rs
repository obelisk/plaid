use std::{collections::HashMap, fmt::Display};

use crate::PlaidFunctionError;

/// Check whether a user belongs to an org
/// ## Arguments
///
/// * `user` - The account to check org membership of
/// * `org` - The org to check if `user` is part of
pub fn check_org_membership_of_user(
    user: impl Display,
    org: impl Display,
) -> Result<bool, PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, check_org_membership_of_user);
    }
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("user", user.to_string());
    params.insert("org", org.to_string());

    let request = serde_json::to_string(&params).unwrap();

    let res = unsafe {
        github_check_org_membership_of_user(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    Ok(res != 0)
}
