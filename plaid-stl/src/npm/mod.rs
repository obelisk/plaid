use serde::{Deserialize, Deserializer, Serialize};

use crate::PlaidFunctionError;

use chrono::{DateTime, Utc};

// All the structs which are also used in the Plaid API

/// Permission that is granted to a team over an npm package
#[derive(Serialize, Deserialize)]
pub enum NpmPackagePermission {
    READ,
    WRITE,
}

impl ToString for NpmPackagePermission {
    fn to_string(&self) -> String {
        match self {
            Self::READ => "read".to_string(),
            Self::WRITE => "write".to_string(),
        }
    }
}

/// Teams in the npm organization
#[derive(Serialize, Deserialize)]
pub enum NpmTeam {
    Admins,
    Developers,
}

impl ToString for NpmTeam {
    fn to_string(&self) -> String {
        match self {
            Self::Admins => "Admins".to_string(),
            Self::Developers => "developers".to_string(),
        }
    }
}

/// Role that a user can have in the npm organization
#[derive(Debug, Deserialize, Serialize)]
pub enum NpmUserRole {
    Member,
    Admin,
    Owner,
}

impl NpmUserRole {
    // We do not implement the TryFrom trait because of the error type. We do not want to carry the
    // NpmError struct here in the STL. Also, we don't care about which error we get from here. If
    // it fails, the caller will know what to do.
    pub fn try_from(value: String) -> Result<Self, ()> {
        match value.as_str() {
            "developer" => Ok(NpmUserRole::Member),
            "admin" => Ok(NpmUserRole::Admin), // untested: I think we do not have Admins
            "super-admin" => Ok(NpmUserRole::Owner),
            _ => Err(()),
        }
    }
}

/// A user in the npm organization
#[derive(Debug, Serialize, Deserialize)]
pub struct NpmUser {
    pub username: String,
    pub role: NpmUserRole,
}

/// Org-level permission granted to a granular token.
///
/// Note: this is incomplete. Other permission levels are possible, but we never needed to add them.
#[derive(Clone, Serialize, Deserialize)]
pub enum GranularTokenOrgsPermission {
    NoAccess,
}

impl ToString for GranularTokenOrgsPermission {
    fn to_string(&self) -> String {
        match self {
            GranularTokenOrgsPermission::NoAccess => "No access".to_string(),
        }
    }
}

/// Package-level permission granted to a granular token.
///
/// Note: this is incomplete. Other permission levels are possible, but we never needed to add them.
#[derive(Clone, Serialize, Deserialize)]
pub enum GranularTokenPackagesAndScopesPermission {
    ReadAndWrite,
}

impl ToString for GranularTokenPackagesAndScopesPermission {
    fn to_string(&self) -> String {
        match self {
            GranularTokenPackagesAndScopesPermission::ReadAndWrite => "Read and write".to_string(),
        }
    }
}

/// TODO I don't exactly know what this means
#[derive(Clone, Serialize, Deserialize)]
pub enum GranularTokenSelectedPackagesAndScopes {
    PackagesAndScopesSome,
}

impl ToString for GranularTokenSelectedPackagesAndScopes {
    fn to_string(&self) -> String {
        match self {
            GranularTokenSelectedPackagesAndScopes::PackagesAndScopesSome => {
                "packagesAndScopesSome".to_string()
            }
        }
    }
}

/// Groups together all the input that we expect from a rule that requests
/// the generation of a granular token. Optional fields will have a default value.
#[derive(Serialize, Deserialize)]
pub struct GranularTokenSpecs {
    pub allowed_ip_ranges: Option<Vec<String>>,
    pub expiration_days: Option<u16>,
    pub orgs_permission: Option<GranularTokenOrgsPermission>,
    pub packages_and_scopes_permission: Option<GranularTokenPackagesAndScopesPermission>,
    pub selected_orgs: Option<Vec<String>>,
    pub selected_packages: Option<Vec<String>>,
    pub selected_packages_and_scopes: Option<GranularTokenSelectedPackagesAndScopes>,
    pub selected_scopes: Option<Vec<String>>,
    pub token_name: String,
    pub token_description: String,
}

impl GranularTokenSpecs {
    /// Provide a name and description for the token, and set all other fields to None, which
    /// will result in default values being used.
    pub fn with_name_and_description(token_name: &str, token_description: &str) -> Self {
        // TODO We could check some things about name and description. E.g., the description
        // should be longer than 8 characters to avoid "test" and "testing".
        Self {
            token_name: token_name.to_string(),
            token_description: token_description.to_string(),
            allowed_ip_ranges: None,
            expiration_days: None,
            orgs_permission: None,
            packages_and_scopes_permission: None,
            selected_orgs: None,
            selected_packages: None,
            selected_packages_and_scopes: None,
            selected_scopes: None,
        }
    }
}

/// An npm token configured on the npm website for the current user
#[derive(Clone, Debug, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct NpmToken {
    token: String,
    token_name: Option<String>,
    pub token_type: Option<String>,
    // Deserialize the date field from the ISO 8601 format
    #[serde(default, deserialize_with = "deserialize_option_timestamp")]
    expires: Option<DateTime<Utc>>,
}

// Custom deserializer for an optional DateTime<Utc>
fn deserialize_option_timestamp<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    // Try to deserialize as a string (ISO 8601 format) or return None if missing
    let opt = Option::<String>::deserialize(deserializer)?;

    // Parse the timestamp string into a DateTime<Utc>, if it's present
    Ok(opt.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    }))
}

#[derive(Serialize, Deserialize)]
pub struct SetTeamPermissionOnPackageParams {
    pub team: String,
    pub package: String,
    pub permission: NpmPackagePermission,
}

#[derive(Serialize, Deserialize)]
pub struct CreateGranularTokenForPackageParams {
    pub package: String,
    pub specs: GranularTokenSpecs,
}

#[derive(Serialize, Deserialize)]
pub struct AddRemoveUserToFromTeamParams {
    pub user: String,
    pub team: NpmTeam,
}

#[derive(Serialize, Deserialize)]
pub struct InviteUserToOrganizationParams {
    pub user: String,
    pub team: Option<NpmTeam>,
}

/// Access level for an npm package
#[derive(Serialize, Deserialize)]
pub enum PkgAccessLevel {
    Restricted,
    Public,
}

impl PkgAccessLevel {
    pub fn to_string(&self) -> String {
        match self {
            PkgAccessLevel::Restricted => "restricted".to_string(),
            PkgAccessLevel::Public => "public".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct PublishEmptyStubParams {
    pub package_name: String,
    pub access_level: PkgAccessLevel,
}

// End of "All the structs which are also used in the Plaid API"

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
///
/// TODO The Plaid API supports specifying another team as the default for the new user. Expose through the STL?
pub fn invite_user_to_organization(user: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, invite_user_to_organization);
    }

    let params = serde_json::to_string(&InviteUserToOrganizationParams {
        user: user.to_string(),
        team: None,
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
pub fn remove_user_from_organization(user: &str) -> Result<(), PlaidFunctionError> {
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

/// Create a granular npm token for a package. The token can be configured through the token_specs parameter.
///
/// If you are not sure about the token configuration, use `create_granular_token_for_package_simple` which only
/// requires specifying a name and a description.
pub fn create_granular_token_for_package(
    package_name: &str,
    token_specs: GranularTokenSpecs,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(npm, create_granular_token_for_package);
    }

    const RETURN_BUFFER_SIZE: usize = 8 * 1024; // 8 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&CreateGranularTokenForPackageParams {
        package: package_name.to_string(),
        specs: token_specs,
    })
    .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res = unsafe {
        npm_create_granular_token_for_package(
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

/// Create a granular npm token for a package, specifying only the token name and a suitable description.
/// Other token configurations default to sensible values.
pub fn create_granular_token_for_package_simple(
    package_name: &str,
    token_name: &str,
    token_description: &str,
) -> Result<String, PlaidFunctionError> {
    let token_specs = GranularTokenSpecs::with_name_and_description(token_name, token_description);
    create_granular_token_for_package(package_name, token_specs)
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
pub fn add_user_to_team(user: &str, team: NpmTeam) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, add_user_to_team);
    }

    let params = serde_json::to_string(&AddRemoveUserToFromTeamParams {
        user: user.to_string(),
        team,
    })
    .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    let res = unsafe { npm_add_user_to_team(params.as_bytes().as_ptr(), params.as_bytes().len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Remove a user from an npm team
pub fn remove_user_from_team(user: &str, team: NpmTeam) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, remove_user_from_team);
    }

    let params = serde_json::to_string(&AddRemoveUserToFromTeamParams {
        user: user.to_string(),
        team,
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
    package_name: &str,
    access_level: Option<PkgAccessLevel>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(npm, publish_empty_stub);
    }

    let access_level = access_level.unwrap_or(PkgAccessLevel::Restricted);

    let params = serde_json::to_string(&PublishEmptyStubParams {
        package_name: package_name.to_string(),
        access_level,
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
    package_name: &str,
    team: NpmTeam,
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
pub fn delete_package(package_name: &str) -> Result<(), PlaidFunctionError> {
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
