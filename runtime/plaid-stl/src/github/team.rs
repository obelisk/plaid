use std::{collections::HashMap, fmt::Display};

use crate::PlaidFunctionError;

// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn add_user_to_team(team: &str, user: &str, org: &str, role: &str) -> Result<(), i32> {
    add_user_to_team_detailed(team, user, org, role).map_err(|_| -4)
}

/// Add a user to a team
/// ## Arguments
///
/// * `team` - The team to add the user to
/// * `user` - The user to add to `team`
/// * `org` - Github organization that `team` exists in
/// * `role` - Role to grant `user` on `team`
pub fn add_user_to_team_detailed(
    team: &str,
    user: &str,
    org: &str,
    role: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, add_user_to_team);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("user", user);
    params.insert("team_slug", team);
    params.insert("org", org);
    params.insert("role", role);

    let params = serde_json::to_string(&params).unwrap();
    let res =
        unsafe { github_add_user_to_team(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn remove_user_from_team(team: &str, user: &str, org: &str) -> Result<(), i32> {
    remove_user_from_team_detailed(team, user, org).map_err(|_| -4)
}

/// Remove a user from a team
/// ## Arguments
///
/// * `team` - The team to remove the user from
/// * `user` - The user to remove from `team`
/// * `org` - Github organization that `team` exists in
pub fn remove_user_from_team_detailed(
    team: &str,
    user: &str,
    org: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_user_from_team);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("user", user);
    params.insert("team_slug", team);
    params.insert("org", org);

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_remove_user_from_team(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Add a repo to a GH team, with a given permission.
/// For more details, see https://docs.github.com/en/rest/teams/teams?apiVersion=2022-11-28#add-or-update-team-repository-permissions
pub fn add_repo_to_team(
    org: impl Display,
    repo: impl Display,
    team_slug: impl Display,
    permission: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, add_repo_to_team);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("org", org.to_string());
    params.insert("team_slug", team_slug.to_string());
    params.insert("repo", repo.to_string());
    params.insert("permission", permission.to_string());

    let params = serde_json::to_string(&params).unwrap();
    let res =
        unsafe { github_add_repo_to_team(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Remove a repo from a GH team.
/// For more details, see https://docs.github.com/en/rest/teams/teams?apiVersion=2022-11-28#remove-a-repository-from-a-team
pub fn remove_repo_from_team(
    org: impl Display,
    repo: impl Display,
    team_slug: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_repo_from_team);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("org", org.to_string());
    params.insert("team_slug", team_slug.to_string());
    params.insert("repo", repo.to_string());

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_remove_repo_from_team(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}
