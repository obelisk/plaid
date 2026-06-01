use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{
    github::{GithubApiWrapper, WeeklyCommits},
    PlaidFunctionError,
};

#[derive(Serialize, Deserialize)]
pub struct GetWeeklyCommitCountParams {
    pub owner: String,
    pub repo: String,
}

/// Get the weekly commit count on a given repo.
/// For more details, see https://docs.github.com/en/rest/metrics/statistics?apiVersion=2022-11-28#get-the-weekly-commit-count
pub fn get_weekly_commit_count(
    client_id: impl Display,
    owner: impl Display,
    repo: impl Display,
) -> Result<WeeklyCommits, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, get_weekly_commit_count);
    }

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = GetWeeklyCommitCountParams {
        owner: owner.to_string(),
        repo: repo.to_string(),
    };
    let wrapper = GithubApiWrapper {
        client_id: client_id.to_string(),
        params,
    };

    let params = serde_json::to_string(&wrapper).unwrap();

    let res = unsafe {
        github_get_weekly_commit_count(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    let response_body =
        String::from_utf8(return_buffer).map_err(|_| PlaidFunctionError::InternalApiError)?;
    let response_body = serde_json::from_str::<WeeklyCommits>(&response_body)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(response_body)
}
