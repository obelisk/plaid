use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{
    github::{GitHubRepoTeam, GithubApiWrapper},
    PlaidFunctionError,
};

#[derive(Serialize, Deserialize)]
pub struct AddUserToTeamParams {
    pub org: String,
    pub team_slug: String,
    pub user: String,
    pub role: String,
}

#[deprecated(
    note = "This function is deprecated and will be removed soon. Please use add_user_to_team_detailed instead."
)]
pub fn add_user_to_team(
    client_id: impl Display,
    team: &str,
    user: &str,
    org: &str,
    role: &str,
) -> Result<(), i32> {
    add_user_to_team_detailed(client_id, team, user, org, role).map_err(|_| -4)
}

/// Add a user to a team
/// ## Arguments
///
/// * `team` - The team to add the user to
/// * `user` - The user to add to `team`
/// * `org` - Github organization that `team` exists in
/// * `role` - Role to grant `user` on `team`
pub fn add_user_to_team_detailed(
    client_id: impl Display,
    team: &str,
    user: &str,
    org: &str,
    role: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, add_user_to_team);
    }

    let params = AddUserToTeamParams {
        org: org.to_string(),
        team_slug: team.to_string(),
        user: user.to_string(),
        role: role.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();
    let res =
        unsafe { github_add_user_to_team(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct RemoveUserFromTeamParams {
    pub org: String,
    pub team_slug: String,
    pub user: String,
}

#[deprecated(
    note = "This function is deprecated and will be removed soon. Please use remove_user_from_team_detailed instead."
)]
pub fn remove_user_from_team(
    client_id: impl Display,
    team: &str,
    user: &str,
    org: &str,
) -> Result<(), i32> {
    remove_user_from_team_detailed(client_id, team, user, org).map_err(|_| -4)
}

/// Remove a user from a team
/// ## Arguments
///
/// * `team` - The team to remove the user from
/// * `user` - The user to remove from `team`
/// * `org` - Github organization that `team` exists in
pub fn remove_user_from_team_detailed(
    client_id: impl Display,
    team: &str,
    user: &str,
    org: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_user_from_team);
    }

    let params = RemoveUserFromTeamParams {
        org: org.to_string(),
        team_slug: team.to_string(),
        user: user.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();
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

#[derive(Serialize, Deserialize)]
pub struct AddRepoToTeamParams {
    pub org: String,
    pub team_slug: String,
    pub repo: String,
    pub permission: String,
}

/// Add a repo to a GH team, with a given permission.
/// For more details, see https://docs.github.com/en/rest/teams/teams?apiVersion=2022-11-28#add-or-update-team-repository-permissions
pub fn add_repo_to_team(
    client_id: impl Display,
    org: impl Display,
    repo: impl Display,
    team_slug: impl Display,
    permission: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, add_repo_to_team);
    }

    let params = AddRepoToTeamParams {
        org: org.to_string(),
        team_slug: team_slug.to_string(),
        repo: repo.to_string(),
        permission: permission.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();
    let res =
        unsafe { github_add_repo_to_team(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct RemoveRepoFromTeamParams {
    pub org: String,
    pub team_slug: String,
    pub repo: String,
}

/// Remove a repo from a GH team.
/// For more details, see https://docs.github.com/en/rest/teams/teams?apiVersion=2022-11-28#remove-a-repository-from-a-team
pub fn remove_repo_from_team(
    client_id: impl Display,
    org: impl Display,
    repo: impl Display,
    team_slug: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_repo_from_team);
    }

    let params = RemoveRepoFromTeamParams {
        org: org.to_string(),
        team_slug: team_slug.to_string(),
        repo: repo.to_string(),
    };

    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();
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

#[derive(Serialize, Deserialize)]
pub struct GetRepoTeamsParams {
    pub org: String,
    pub repo: String,
    pub per_page: Option<u8>,
    pub page: Option<u16>,
}

/// Get the teams that have access to a repository.
pub fn get_repo_teams(
    client_id: impl Display,
    org: impl Display,
    repo: impl Display,
) -> Result<Vec<GitHubRepoTeam>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_repo_teams);
    }

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    let mut teams = Vec::<GitHubRepoTeam>::new();
    let mut page = 0;
    loop {
        page += 1;

        let params = GetRepoTeamsParams {
            org: org.to_string(),
            repo: repo.to_string(),
            per_page: None,
            page: Some(page),
        };

        let wrapper = GithubApiWrapper {
            client_id: client_id.to_string(),
            params,
        };

        let request = serde_json::to_string(&wrapper).unwrap();

        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

        let res = unsafe {
            github_get_repo_teams(
                request.as_bytes().as_ptr(),
                request.as_bytes().len(),
                return_buffer.as_mut_ptr(),
                RETURN_BUFFER_SIZE,
            )
        };

        if res < 0 {
            return Err(res.into());
        }

        return_buffer.truncate(res as usize);
        // This should be safe because unless the Plaid runtime is expressly trying
        // to mess with us, this came from a String in the API module.
        let this_page = String::from_utf8(return_buffer).unwrap();
        if this_page == "[]" {
            break;
        }
        teams.extend(
            serde_json::from_str::<Vec<GitHubRepoTeam>>(&this_page)
                .map_err(|_| PlaidFunctionError::InternalApiError)?,
        );
    }

    Ok(teams)
}
