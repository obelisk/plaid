use std::collections::HashMap;

use serde::Serialize;

use crate::PlaidFunctionError;

pub enum ReviewPatAction {
    Approve,
    Deny,
}

// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn add_user_to_team(team: &str, user: &str, org: &str, role: &str) -> Result<(), i32> {
    add_user_to_team_detailed(team, user, org, role).map_err(|_| -4)
}

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

// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn remove_user_from_repo(repo: &str, user: &str) -> Result<(), i32> {
    remove_user_from_repo_detailed(repo, user).map_err(|_| -4)
}

pub fn remove_user_from_repo_detailed(repo: &str, user: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, remove_user_from_repo);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("user", user);
    params.insert("repo", repo);

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_remove_user_from_repo(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// TODO: Do not use this function, it is deprecated and will be removed soon
pub fn add_user_to_repo(repo: &str, user: &str, permission: Option<&str>) -> Result<(), i32> {
    add_user_to_repo_detailed(repo, user, permission).map_err(|_| -4)
}

pub fn add_user_to_repo_detailed(
    repo: &str,
    user: &str,
    permission: Option<&str>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, add_user_to_repo);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("user", user);
    params.insert("repo", repo);
    if let Some(permission) = permission {
        params.insert("permission", permission);
    }

    let params = serde_json::to_string(&params).unwrap();
    let res =
        unsafe { github_add_user_to_repo(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

pub fn make_graphql_query(
    query_name: &str,
    variables: HashMap<String, String>,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, make_graphql_query);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB

    #[derive(Serialize)]
    struct Request {
        query_name: String,
        variables: HashMap<String, String>,
    }

    let request = Request {
        query_name: query_name.to_owned(),
        variables,
    };

    let query = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_make_graphql_query(
            query.as_bytes().as_ptr(),
            query.as_bytes().len(),
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
    Ok(String::from_utf8(return_buffer).unwrap())
}

pub fn make_advanced_graphql_query(
    query_name: &str,
    variables: HashMap<String, serde_json::Value>,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, make_advanced_graphql_query);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB

    #[derive(Serialize)]
    struct Request {
        query_name: String,
        variables: HashMap<String, serde_json::Value>,
    }

    let request = Request {
        query_name: query_name.to_owned(),
        variables,
    };

    let query = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_make_advanced_graphql_query(
            query.as_bytes().as_ptr(),
            query.as_bytes().len(),
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
    Ok(String::from_utf8(return_buffer).unwrap())
}

pub fn fetch_commit(user: &str, repo: &str, commit: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, fetch_commit);
    }
    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    #[derive(Serialize)]
    struct Request<'a> {
        user: &'a str,
        repo: &'a str,
        commit: &'a str,
    }

    let request = Request { user, repo, commit };

    let request = serde_json::to_string(&request).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_fetch_commit(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
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
    Ok(String::from_utf8(return_buffer).unwrap())
}

pub fn list_fpat_requests_for_org(org: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, list_fpat_requests_for_org);
    }
    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("org", org);

    let request = serde_json::to_string(&params).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_list_fpat_requests_for_org(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
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
    Ok(String::from_utf8(return_buffer).unwrap())
}

pub fn review_fpat_requests_for_org(
    org: String,
    pat_request_ids: &[u64],
    action: ReviewPatAction,
    reason: String,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(github, review_fpat_requests_for_org);
    }
    #[derive(Serialize)]
    struct Request {
        org: String,
        pat_request_ids: Vec<u64>,
        action: String,
        reason: String,
    }

    let request = Request {
        org,
        pat_request_ids: pat_request_ids.to_vec(),
        action: match action {
            ReviewPatAction::Approve => "approve".to_string(),
            ReviewPatAction::Deny => "deny".to_string(),
        },
        reason,
    };

    let request = serde_json::to_string(&request).unwrap();

    let res = unsafe {
        github_review_fpat_requests_for_org(request.as_bytes().as_ptr(), request.as_bytes().len())
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    match res {
        0 => Ok(()),
        x => Err(x.into()),
    }
}
