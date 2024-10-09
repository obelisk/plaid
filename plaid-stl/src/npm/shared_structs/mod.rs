//! This module contains all structs which are shared between the STL and the runtime API.
//! These are used for ensuring consistent serialization / deserialization across the
//! host / guest boundary.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt::Display;

/// Permission that is granted to a team over an npm package
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum NpmPackagePermission {
    READ,
    WRITE,
}

impl Display for NpmPackagePermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            Self::READ => "read".to_string(),
            Self::WRITE => "write".to_string(),
        };
        write!(f, "{}", out)
    }
}

impl TryFrom<String> for NpmPackagePermission {
    type Error = ();

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "read" => Ok(NpmPackagePermission::READ),
            "write" => Ok(NpmPackagePermission::WRITE),
            _ => Err(()),
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

impl TryFrom<String> for NpmUserRole {
    type Error = ();

    fn try_from(value: String) -> Result<Self, Self::Error> {
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
#[derive(Clone, Serialize, Deserialize)]
pub enum GranularTokenOrgsPermission {
    NoAccess,
    ReadOnly,
    ReandAndWrite,
}

impl Display for GranularTokenOrgsPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            GranularTokenOrgsPermission::NoAccess => "No access".to_string(),
            GranularTokenOrgsPermission::ReadOnly => "Read only".to_string(),
            GranularTokenOrgsPermission::ReandAndWrite => "Read and write".to_string(),
        };
        write!(f, "{}", out)
    }
}

/// Package-level permission granted to a granular token.
#[derive(Clone, Serialize, Deserialize)]
pub enum GranularTokenPackagesAndScopesPermission {
    NoAccess,
    ReadOnly,
    ReadAndWrite,
}

impl Display for GranularTokenPackagesAndScopesPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            GranularTokenPackagesAndScopesPermission::NoAccess => "No access".to_string(),
            GranularTokenPackagesAndScopesPermission::ReadOnly => "Read only".to_string(),
            GranularTokenPackagesAndScopesPermission::ReadAndWrite => "Read and write".to_string(),
        };
        write!(f, "{}", out)
    }
}

/// Scopes granted to the token
#[derive(Clone, Serialize, Deserialize)]
pub enum GranularTokenSelectedPackagesAndScopes {
    PackagesAll,
    PackagesAndScopesSome,
}

impl Display for GranularTokenSelectedPackagesAndScopes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            GranularTokenSelectedPackagesAndScopes::PackagesAll => "packagesAll".to_string(),
            GranularTokenSelectedPackagesAndScopes::PackagesAndScopesSome => {
                "packagesAndScopesSome".to_string()
            }
        };
        write!(f, "{}", out)
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
    pub fn with_name_and_description(
        token_name: impl Display,
        token_description: impl Display,
    ) -> Self {
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
pub struct NpmToken {
    /// ID assigned by npm to the token.
    /// Note: only granular tokens have this.
    pub id: Option<String>,
    pub token: String,
    pub token_name: Option<String>,
    pub token_type: Option<String>,
    // Deserialize the date field from the ISO 8601 format
    #[serde(default, deserialize_with = "deserialize_option_timestamp")]
    pub expires: Option<DateTime<Utc>>,
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
pub struct CreateGranularTokenForPackagesParams {
    pub packages: Vec<String>,
    pub specs: GranularTokenSpecs,
}

#[derive(Serialize, Deserialize)]
pub struct DeleteTokenParams {
    pub token_id: String
}

#[derive(Serialize, Deserialize)]
pub struct AddRemoveUserToFromTeamParams {
    pub user: String,
    pub team: String,
}

#[derive(Serialize, Deserialize)]
pub struct InviteUserToOrganizationParams {
    pub user: String,
    pub team: Option<String>,
}

/// Access level for an npm package
#[derive(Serialize, Deserialize)]
pub enum PkgAccessLevel {
    Restricted,
    Public,
}

impl Display for PkgAccessLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = match self {
            PkgAccessLevel::Restricted => "restricted".to_string(),
            PkgAccessLevel::Public => "public".to_string(),
        };
        write!(f, "{}", out)
    }
}

#[derive(Serialize, Deserialize)]
pub struct PublishEmptyStubParams {
    pub package_name: String,
    pub access_level: PkgAccessLevel,
    pub repo_name: String,
}

#[derive(Serialize, Deserialize)]
pub struct ListPackagesWithTeamPermissionParams {
    pub team: String,
    pub permission: NpmPackagePermission,
}

#[derive(Serialize, Deserialize)]
pub struct GetTokenDetailsParams {
    pub token_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct GranularTokenDetails {
    #[serde(rename = "tokenName")]
    pub token_name: Option<String>,
    #[serde(rename = "tokenID")]
    pub token_id: Option<String>,
    #[serde(rename = "tokenDescription")]
    pub token_description: Option<String>,
    #[serde(rename = "packagesAndScopesPermission")]
    pub packages_and_scopes_permission: String,
    #[serde(rename = "selectedPackagesAndScopes")]
    pub selected_packages_and_scopes: String,
    #[serde(rename = "selectedPackages")]
    pub selected_packages: Vec<String>,
    expired: bool,
    #[serde(rename = "selectedScopes")]
    pub selected_scopes: Vec<String>,
    #[serde(rename = "selectedOrgs")]
    pub selected_orgs: Vec<String>,
}
