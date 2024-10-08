pub mod shared_structs;

use std::fmt::Display;

use crate::PlaidFunctionError;
use serde::Deserialize;

use shared_structs::*;

/// Retrieve a list of users in the npm organization
pub fn get_org_user_list() -> Result<Vec<NpmUser>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(npm, get_org_user_list);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = "".to_string();

    let res = unsafe {
        npm_get_org_user_list(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
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

    serde_json::from_str::<Vec<NpmUser>>(&res).map_err(|_| PlaidFunctionError::InternalApiError)
}

/// Retrieve a list of users in the npm organization that do not have 2FA enabled
pub fn get_org_users_without_2fa() -> Result<Vec<NpmUser>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(npm, get_org_users_without_2fa);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = "".to_string();

    let res = unsafe {
        npm_get_org_users_without_2fa(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
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

    serde_json::from_str::<Vec<NpmUser>>(&res).map_err(|_| PlaidFunctionError::InternalApiError)
}

/// Invite a user to join the npm organization. If the user accepts the invite, they will be added
/// to the default team "developers".
pub fn invite_user_to_organization(
    user: impl Display,
    team: Option<impl Display>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, invite_user_to_organization);
    }

    let params = serde_json::to_string(&InviteUserToOrganizationParams {
        user: user.to_string(),
        team: team.map(|t| t.to_string()),
    })
    .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res = unsafe {
        npm_invite_user_to_organization(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Remove a user from the npm organization
pub fn remove_user_from_organization(user: impl Display) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, remove_user_from_organization);
    }

    let params = user.to_string();

    let res = unsafe {
        npm_remove_user_from_organization(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Create a granular npm token for a list of packages. The token can be configured through the token_specs parameter.
///
/// If you are not sure about the token configuration, use `create_granular_token_for_packages_simple` which only
/// requires specifying a name and a description.
pub fn create_granular_token_for_packages(
    package_names: impl IntoIterator<Item = impl Display>,
    token_specs: GranularTokenSpecs,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(npm, create_granular_token_for_packages);
    }

    const RETURN_BUFFER_SIZE: usize = 8 * 1024; // 8 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let package_names = package_names.into_iter().map(|v| v.to_string()).collect();

    let params = serde_json::to_string(&CreateGranularTokenForPackagesParams {
        packages: package_names,
        specs: token_specs,
    })
    .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res = unsafe {
        npm_create_granular_token_for_packages(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
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
    Ok(String::from_utf8(return_buffer).unwrap())
}

/// Create a granular npm token for a list of packages, specifying only the token name and a suitable description.
/// Other token configurations default to sensible values.
pub fn create_granular_token_for_packages_simple(
    package_names: impl IntoIterator<Item = impl Display>,
    token_name: impl Display,
    token_description: impl Display,
) -> Result<String, PlaidFunctionError> {
    let token_specs = GranularTokenSpecs::with_name_and_description(token_name, token_description);
    create_granular_token_for_packages(package_names, token_specs)
}

/// Delete a granular token with given ID from the npm website
pub fn delete_granular_token(token_id: impl Display) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, delete_granular_token);
    }

    let params = serde_json::to_string(&DeleteTokenParams {
        token_id: token_id.to_string(),
    })
    .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res =
        unsafe { npm_delete_granular_token(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    if res < 0 {
        return Err(res.into());
    }
    Ok(())
}

/// Renew an npm granular token with given token name (which is a unique identifier for npm tokens).
///
/// ! Important note ! - This creates a new token with the same publish scope as the old one and _overwrites_
/// the token on the npm website. From the original token, only the "selected packages" are kept and
/// transferred to the new token. This means scopes, organizations, etc., are currently not supported
/// and are lost in case of a renewal.
pub fn renew_granular_token(token_name: impl Display) -> Result<String, PlaidFunctionError> {
    // 1. Pull info for existing token
    let token_details = list_granular_tokens()?
        .iter()
        .find(|t| match &t.token_name {
            None => false,
            Some(n) => *n == token_name.to_string(),
        })
        .ok_or(PlaidFunctionError::InternalApiError)?
        .get_details()?;

    // 2. Delete existing token
    delete_granular_token(
        token_details
            .token_id
            .ok_or(PlaidFunctionError::InternalApiError)?,
    )?;

    // 3. Create new token with same info as previous token
    let publish_scope: Vec<String> = token_details
        .selected_packages
        .iter()
        // remove scope "@xxxxxx/" because the API does not want it
        // (it will be added automatically later)
        .map(|v| match v.find('/') {
            None => v.to_string(),
            Some(i) => v.split_at(i + 1).1.to_string(),
        })
        .collect();
    let new_token = create_granular_token_for_packages_simple(
        publish_scope,
        token_details
            .token_name
            .ok_or(PlaidFunctionError::InternalApiError)?,
        token_details
            .token_description
            .unwrap_or("Missing description".to_string()),
    )?;

    Ok(new_token)
}

/// Retrieve a list of all granular tokens configured for the service account
pub fn list_granular_tokens() -> Result<Vec<NpmToken>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(npm, list_granular_tokens);
    }

    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = "".to_string();

    let res = unsafe {
        npm_list_granular_tokens(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
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

    serde_json::from_str::<Vec<NpmToken>>(&res).map_err(|_| PlaidFunctionError::InternalApiError)
}

/// Add a user to an npm team
pub fn add_user_to_team(user: impl Display, team: impl Display) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, add_user_to_team);
    }

    let params = serde_json::to_string(&AddRemoveUserToFromTeamParams {
        user: user.to_string(),
        team: team.to_string(),
    })
    .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res = unsafe { npm_add_user_to_team(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Remove a user from an npm team
pub fn remove_user_from_team(
    user: impl Display,
    team: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, remove_user_from_team);
    }

    let params = serde_json::to_string(&AddRemoveUserToFromTeamParams {
        user: user.to_string(),
        team: team.to_string(),
    })
    .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res =
        unsafe { npm_remove_user_from_team(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Publish an empty npm package, to be later updated.
/// If an access level is not specified (i.e., None is passed), it defaults to "restricted".
pub fn publish_empty_stub(
    package_name: impl Display,
    access_level: Option<PkgAccessLevel>,
    github_repo: impl Display,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, publish_empty_stub);
    }

    let access_level = access_level.unwrap_or(PkgAccessLevel::Restricted);

    let params = serde_json::to_string(&PublishEmptyStubParams {
        package_name: package_name.to_string(),
        access_level,
        repo_name: github_repo.to_string(),
    })
    .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res =
        unsafe { npm_publish_empty_stub(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Set permissions for a team on a specific npm package
pub fn set_team_permission_on_package(
    package_name: impl Display,
    team: impl Display,
    permission: NpmPackagePermission,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, set_team_permission_on_package);
    }

    let params = serde_json::to_string(&SetTeamPermissionOnPackageParams {
        team: team.to_string(),
        package: package_name.to_string(),
        permission,
    })
    .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res = unsafe {
        npm_set_team_permission_on_package(params.as_bytes().as_ptr(), params.as_bytes().len())
    };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Delete a package under the npm organization from the npm registry.
///
/// Note: The package name should be unscoped. If you are trying to delete
/// @scope/package_name, then you should pass only "package_name". The scope is
/// preconfigured in the client and will be added automatically.
pub fn delete_package(package_name: impl Display) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, delete_package);
    }

    let params = package_name.to_string();

    let res = unsafe { npm_delete_package(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Return a list of npm packages over which a team has a certain permission (read or write)
pub fn list_packages_with_team_permission(
    team: impl Display,
    permission: NpmPackagePermission,
) -> Result<Vec<String>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(npm, list_packages_with_team_permission);
    }

    let params = serde_json::to_string(&ListPackagesWithTeamPermissionParams {
        team: team.to_string(),
        permission,
    })
    .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        npm_list_packages_with_team_permission(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
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

    // The response that comes back contains package name and permission.
    // However, we only care about the name so we keep only that.
    #[derive(Deserialize)]
    struct Package {
        package_name: String,
    }

    let v: Vec<String> = serde_json::from_str::<Vec<Package>>(&res)
        .map_err(|_| PlaidFunctionError::InternalApiError)?
        .iter()
        .map(|v| v.package_name.clone())
        .collect();
    Ok(v)
}

impl NpmToken {
    /// Return details about this token
    pub fn get_details(&self) -> Result<GranularTokenDetails, PlaidFunctionError> {
        extern "C" {
            new_host_function_with_error_buffer!(npm, get_token_details);
        }

        const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

        let params = serde_json::to_string(&GetTokenDetailsParams {
            token_id: self
                .id
                .as_ref()
                .ok_or(PlaidFunctionError::ErrorCouldNotSerialize)?
                .to_string(),
        })
        .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

        let res = unsafe {
            npm_get_token_details(
                params.as_bytes().as_ptr(),
                params.as_bytes().len(),
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

        serde_json::from_str::<GranularTokenDetails>(&res)
            .map_err(|_| PlaidFunctionError::InternalApiError)
    }

    /// Renew this token, keeping the same publish scope
    pub fn renew(&self) -> Result<String, PlaidFunctionError> {
        renew_granular_token(
            self.token_name
                .as_ref()
                .ok_or(PlaidFunctionError::InternalApiError)?,
        )
    }
}
