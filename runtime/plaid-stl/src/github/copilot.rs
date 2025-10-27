use std::collections::HashMap;

use serde::Serialize;

use crate::{
    github::{
        CopilotAddUsersResponse, CopilotRemoveUsersResponse, CopilotSeat, CopilotSeatsResult,
    },
    PlaidFunctionError,
};

/// List seats in org's Copilot subscription, paginated
/// ## Arguments
///
/// * `org` - The org owning the subscription
/// * `per_page` - The number of results per page (max 100)
/// * `page` - The page number of the results to fetch.
pub fn list_copilot_subscription_seats_by_page(
    org: &str,
    per_page: Option<u64>,
    page: Option<u64>,
) -> Result<Vec<CopilotSeat>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, list_seats_in_org_copilot);
    }

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("org", org.to_string());
    if let Some(per_page) = per_page {
        params.insert("per_page", per_page.to_string());
    }
    if let Some(page) = page {
        params.insert("page", page.to_string());
    }

    let request = serde_json::to_string(&params).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        github_list_seats_in_org_copilot(
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
    let res = String::from_utf8(return_buffer).unwrap();

    let res = serde_json::from_str::<CopilotSeatsResult>(&res)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(res.seats)
}

/// List all seats in org's Copilot subscription
/// ## Arguments
///
/// * `org` - The org owning the subscription
pub fn list_all_copilot_subscription_seats(
    org: &str,
) -> Result<Vec<CopilotSeat>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, list_seats_in_org_copilot);
    }

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("org", org.to_string());
    // 100 is max per page
    params.insert("per_page", "100".to_string());

    let mut seats = Vec::<CopilotSeat>::new();
    let mut page = 0;

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB

    loop {
        page += 1;
        params.insert("page", page.to_string());

        let request = serde_json::to_string(&params).unwrap();

        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

        let res = unsafe {
            github_list_seats_in_org_copilot(
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

        let this_page = serde_json::from_str::<CopilotSeatsResult>(&this_page)
            .map_err(|_| PlaidFunctionError::InternalApiError)?;

        if this_page.seats.len() == 0 {
            break;
        }

        seats.extend(this_page.seats);
    }

    Ok(seats)
}

/// Add a user to the org's Copilot subscription
/// ## Arguments
///
/// * `org` - The org owning the subscription
/// * `user` - The user to add to Copilot subscription
pub fn add_user_to_copilot_subscription(
    org: &str,
    user: &str,
) -> Result<CopilotAddUsersResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, add_users_to_org_copilot);
    }
    #[derive(Serialize)]
    struct Params<'a> {
        org: &'a str,
        selected_usernames: Vec<&'a str>,
    }
    let params = Params {
        org,
        selected_usernames: vec![user],
    };

    const RETURN_BUFFER_SIZE: usize = 1024; // 1 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_add_users_to_org_copilot(
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
    let response_body = serde_json::from_str::<CopilotAddUsersResponse>(&response_body)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(response_body)
}

/// Remove a user from the org's Copilot subscription
/// ## Arguments
///
/// * `org` - The org owning the subscription
/// * `user` - The user to remove from Copilot subscription
pub fn remove_user_from_copilot_subscription(
    org: &str,
    user: &str,
) -> Result<CopilotRemoveUsersResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, remove_users_from_org_copilot);
    }

    #[derive(Serialize)]
    struct Params<'a> {
        org: &'a str,
        selected_usernames: Vec<&'a str>,
    }
    let params = Params {
        org,
        selected_usernames: vec![user],
    };

    const RETURN_BUFFER_SIZE: usize = 1024; // 1 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_remove_users_from_org_copilot(
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
    let response_body = serde_json::from_str::<CopilotRemoveUsersResponse>(&response_body)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(response_body)
}

/// Remove multiple users from the org's Copilot subscription
/// ## Arguments
///
/// * `org` - The org owning the subscription
/// * `users` - The list of users to remove from Copilot subscription
pub fn remove_users_from_copilot_subscription(
    org: &str,
    users: Vec<&str>,
) -> Result<CopilotRemoveUsersResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, remove_users_from_org_copilot);
    }

    #[derive(Serialize)]
    struct Params<'a> {
        org: &'a str,
        selected_usernames: Vec<&'a str>,
    }
    let params = Params {
        org,
        selected_usernames: users,
    };

    const RETURN_BUFFER_SIZE: usize = 1024; // 1 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        github_remove_users_from_org_copilot(
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
    let response_body = serde_json::from_str::<CopilotRemoveUsersResponse>(&response_body)
        .map_err(|_| PlaidFunctionError::InternalApiError)?;

    Ok(response_body)
}
