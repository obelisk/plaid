use reqwest::cookie::CookieStore;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use totp_rs::{Algorithm, Secret, TOTP};
use url::Url;

use crate::apis::ApiError;

use plaid_stl::npm::shared_structs::*;

use super::{Npm, NpmError};

const NPMJS_COM_URL: &str = "https://www.npmjs.com";

#[derive(Serialize)]
/// Payload sent to npm website to change a team's permissions over a package
struct PermissionChangePayload<'a> {
    csrftoken: &'a str,
    package: &'a str,
    permissions: &'a str,
}

#[derive(Serialize)]
/// Payload sent to npm website to generate a new granular token
struct GenerateGranularTokenPayload<'a> {
    #[serde(rename = "allowedIPRanges")]
    allowed_ip_ranges: Vec<String>,
    csrftoken: &'a str,
    #[serde(rename = "expirationDays")]
    expiration_days: &'a str,
    #[serde(rename = "orgsPermission")]
    orgs_permission: &'a str,
    #[serde(rename = "packagesAndScopesPermission")]
    packages_and_scopes_permission: &'a str,
    #[serde(rename = "selectedOrgs")]
    selected_orgs: Vec<String>,
    #[serde(rename = "selectedPackages")]
    selected_packages: Vec<String>,
    #[serde(rename = "selectedPackagesAndScopes")]
    selected_packages_and_scopes: &'a str,
    #[serde(rename = "selectedScopes")]
    selected_scopes: Vec<String>,
    #[serde(rename = "tokenDescription")]
    token_description: &'a str,
    #[serde(rename = "tokenName")]
    token_name: &'a str,
}

#[derive(Serialize)]
struct InviteUserToOrganizationPayload<'a> {
    csrftoken: &'a str,
    team: &'a str,
    user: HashMap<&'a str, &'a str>,
}

#[derive(Serialize)]
struct AddUserToTeamPayload<'a> {
    csrftoken: &'a str,
    user: &'a str,
}

#[derive(Serialize)]
struct RemoveUserPayload<'a> {
    csrftoken: &'a str,
}

#[derive(Serialize)]
struct DeleteTokenPayload<'a> {
    csrftoken: &'a str,
    tokens: &'a str,
}

impl Npm {
    /// Retrieve the CSRF token from the client's cookie jar
    fn get_csrftoken_from_cookies(&self) -> Result<String, NpmError> {
        let cookies: Vec<String> = self
            .cookie_jar
            .as_ref()
            .cookies(&Url::parse(NPMJS_COM_URL).unwrap())
            .ok_or(NpmError::FailedToGetCsrfTokenFromCookies)?
            .to_str()
            .map_err(|_| NpmError::FailedToGetCsrfTokenFromCookies)?
            .to_string()
            .split("; ")
            .map(|v| v.to_string())
            .collect();
        let mut csrf_token: Option<String> = None;
        for c in cookies {
            if c.starts_with("cs=") {
                csrf_token = Some(
                    c.split("=")
                        .collect::<Vec<&str>>()
                        .get(1)
                        .ok_or(NpmError::FailedToGetCsrfTokenFromCookies)?
                        .to_string(),
                );
                break;
            }
        }
        csrf_token.ok_or(NpmError::FailedToGetCsrfTokenFromCookies)
    }

    /// Execute the web login flow (with username/password + OTP 2FA). As a side effect, this
    /// updates the client's cookie jar, from which we can later extract necessary values.
    async fn login(&self) -> Result<(), NpmError> {
        let response = self
            .client
            .get(format!("{}/login", NPMJS_COM_URL))
            .send()
            .await
            .map_err(|_| NpmError::LoginFlowError)?;
        // Get the CSRF token (which is in a cookie) because we need to send it back later on
        let cs_cookie = match response.cookies().find(|c| c.name() == "cs") {
            None => return Ok(()), // we do not get this cookie if we are _already_ logged in
            Some(c) => c.value().to_string(),
        };

        // Login step 1: send the username and password, together with the CSRF token
        let output = self
            .client
            .post(format!("{}/login", NPMJS_COM_URL))
            .form(&[
                ("username", &self.config.username),
                ("password", &self.config.password),
                ("csrftoken", &cs_cookie),
            ])
            .send()
            .await
            .map(|_| ())
            .map_err(|_| NpmError::LoginFlowError);

        // If we have a configured OTP secret, we proceed with the 2FA flow
        if let Some(ref otp_secret) = self.config.otp_secret {
            let otp_token = TOTP::new(
                Algorithm::SHA1,
                6,
                1,
                30,
                Secret::Encoded(otp_secret.to_string())
                    .to_bytes()
                    .map_err(|_| NpmError::LoginFlowError)?,
            )
            .map_err(|_| NpmError::LoginFlowError)?
            .generate_current()
            .map_err(|_| NpmError::LoginFlowError)?;

            // Login step 2: send the TOTP code to a well-known URL
            let output = self
                .client
                .post(format!("{}/login/otp?next=%2F", NPMJS_COM_URL))
                .form(&[
                    ("otp", &otp_token),
                    ("formName", &"totp".to_string()),
                    ("originalUrl", &"".to_string()),
                    ("csrftoken", &cs_cookie),
                ])
                .send()
                .await
                .map(|_| ())
                .map_err(|_| NpmError::LoginFlowError);
            return output;
        } else {
            // We do not have an OTP secret, so we are done with the login
            return output;
        }
    }

    /// Set a team's permissions over a package
    pub async fn set_team_permission_on_package(
        &self,
        params: &str,
        module: &str,
    ) -> Result<i32, ApiError> {
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let params: SetTeamPermissionOnPackageParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let team = self.validate_npm_team_name(&params.team)?;
        let package = self.validate_npm_package_name(&params.package)?;

        info!(
            "Setting permission [{}] on package [{}] for team [{}] on behalf of [{module}]",
            params.permission.to_string(),
            package,
            team
        );

        // Prepare the request body
        let csrf_token = self
            .get_csrftoken_from_cookies()
            .map_err(|_| ApiError::NpmError(NpmError::WrongClientStatus))?;
        let payload = PermissionChangePayload {
            package: &format!("@{}/{}", self.config.npm_scope, package),
            permissions: &params.permission.to_string(),
            csrftoken: &csrf_token,
        };
        self.client
            .post(format!(
                "{}/settings/{}/teams/team/{}/access",
                NPMJS_COM_URL, self.config.npm_scope, team
            ))
            .json(&payload)
            .send()
            .await
            .map(|_| Ok(0))
            .map_err(|_| ApiError::NpmError(NpmError::PermissionChangeError))?
    }

    /// Create a granular token for a package, with given name and description.
    ///
    /// By default, the token has the following features:
    /// * read/write permission
    /// * expires after 365 days
    /// * scoped to the given packages
    pub async fn create_granular_token_for_packages(
        &self,
        params: &str,
        module: &str,
    ) -> Result<String, ApiError> {
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let params: CreateGranularTokenForPackagesParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let packages = params
            .packages
            .iter()
            .map(|v| self.validate_npm_package_name(v).map(|v| v.to_string()))
            .collect::<Result<Vec<String>, ApiError>>()?;
        let specs = self.validate_granular_token_specs(&params.specs)?;

        info!(
            "Creating npm granular token for packages [{:?}] on behalf of [{module}]",
            packages
        );

        let scoped_packages: Vec<String> = packages
            .iter()
            .map(|v| format!("@{}/{}", self.config.npm_scope, v))
            .collect();
        // Prepare the request body
        let csrf_token = self
            .get_csrftoken_from_cookies()
            .map_err(|_| ApiError::NpmError(NpmError::WrongClientStatus))?;
        let payload = GenerateGranularTokenPayload {
            allowed_ip_ranges: params
                .specs
                .allowed_ip_ranges
                .clone()
                .unwrap_or(vec!["".to_string()]),
            csrftoken: &csrf_token,
            expiration_days: &specs.expiration_days.unwrap_or(365).to_string(),
            orgs_permission: &params
                .specs
                .orgs_permission
                .clone()
                .map_or("No access".to_string(), |v| v.to_string()),
            packages_and_scopes_permission: &params
                .specs
                .packages_and_scopes_permission
                .clone()
                .map_or("Read and write".to_string(), |v| v.to_string()),
            selected_orgs: specs.selected_orgs.clone().unwrap_or(vec![]),
            selected_packages: scoped_packages,
            selected_packages_and_scopes: &params
                .specs
                .selected_packages_and_scopes
                .clone()
                .map_or("packagesAndScopesSome".to_string(), |v| v.to_string()),
            selected_scopes: specs.selected_scopes.clone().unwrap_or(vec![]),
            token_description: &specs.token_description,
            token_name: &specs.token_name,
        };
        let response = self
            .client
            .post(format!(
                "{}/settings/{}/tokens/new-gat",
                NPMJS_COM_URL, self.config.username
            ))
            .json(&payload)
            .header("X-Spiferack", "1") // to get JSON instead of HTML
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::TokenGenerationError))?;

        // The new token is in the JSON response, under the key "newToken"
        response
            .json::<Value>()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::TokenGenerationError))?
            .get("newToken")
            .map(|v| Ok(v.to_string()))
            .ok_or(ApiError::NpmError(NpmError::TokenGenerationError))?
    }

    /// Delete a granular token from the npm website
    pub async fn delete_granular_token(&self, params: &str, module: &str) -> Result<i32, ApiError> {
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let params: DeleteTokenParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let token_id = self.validate_token_id(&params.token_id)?;

        info!("Deleting npm granular token with ID [{token_id}] on behalf of [{module}]");

        // Prepare the request body
        let csrf_token = self
            .get_csrftoken_from_cookies()
            .map_err(|_| ApiError::NpmError(NpmError::WrongClientStatus))?;
        let payload = DeleteTokenPayload {
            csrftoken: &csrf_token,
            tokens: token_id,
        };
        self.client
            .post(format!(
                "{}/settings/{}/tokens/delete",
                NPMJS_COM_URL, self.config.username
            ))
            .json(&payload)
            .header("X-Spiferack", "1") // to get JSON instead of HTML
            .send()
            .await
            .map(|_| Ok(0))
            .map_err(|_| ApiError::NpmError(NpmError::TokenDeletionError))?
    }

    /// Retrieve a list of granular tokens for the account whose credentials have been configured for this client.
    ///
    /// Note: only granular tokens are returned. Other types of tokens (publish, automation, etc.) are filtered out.
    pub async fn list_granular_tokens(&self, _: &str, module: &str) -> Result<String, ApiError> {
        info!("Listing npm granular tokens on behalf of [{module}]");
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let response = self
            .client
            .get(format!(
                "{}/settings/{}/tokens",
                NPMJS_COM_URL, self.config.username
            ))
            .header("X-Spiferack", "1") // to get JSON instead of HTML
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToListGranularTokens))?;

        // Tokens are returned in a JSON structure under "list > objects". We first deserialize to a
        // generic Value, then get "list > objects" and convert to a vector of NpmTokens. Finally, we
        // keep only granular tokens and return the result.
        let granular_tokens = serde_json::from_value::<Vec<NpmToken>>(
            response
                .json::<Value>()
                .await
                .map_err(|_| ApiError::NpmError(NpmError::FailedToListGranularTokens))?
                .get("list")
                .ok_or(ApiError::NpmError(NpmError::FailedToListGranularTokens))?
                .get("objects")
                .ok_or(ApiError::NpmError(NpmError::FailedToListGranularTokens))?
                .clone(),
        )
        .map_err(|_| ApiError::NpmError(NpmError::FailedToListGranularTokens))?
        .iter()
        .filter(|v| v.token_type == Some("granular".to_string()))
        .cloned()
        .collect::<Vec<NpmToken>>();

        serde_json::to_string(&granular_tokens)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToListGranularTokens))
    }

    /// Delete a package from the npm registry.
    ///
    /// Note: The package name should be unscoped. If you are trying to delete
    /// @scope/package_name, then you should pass only "package_name". The scope is
    /// preconfigured in the client and will be added automatically.
    pub async fn delete_package(&self, package: &str, module: &str) -> Result<i32, ApiError> {
        let package = self.validate_npm_package_name(package)?;
        info!("Deleting npm package [{package}] on behalf of [{module}]");
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        // Step 1. Make a GET request to /delete in order to retrieve the dsrManifestHash.
        // This will later be sent as form data when actually performing the deletion.
        let response = self
            .client
            .get(format!(
                "{}/package/%40{}%2F{}/delete",
                NPMJS_COM_URL, self.config.npm_scope, package
            ))
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToDeletePackage))?;
        let response_text = response
            .text()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToDeletePackage))?;
        let dsr_manifest_hash = self
            .validate_and_extract_dsr_manifest_hash(&response_text)?
            .to_string();

        // Step 2. Perform the actual package deletion with a POST request to /delete
        let scoped_package_name = format!("@{}/{}", self.config.npm_scope, package);
        let csrf_token = self
            .get_csrftoken_from_cookies()
            .map_err(|_| ApiError::NpmError(NpmError::WrongClientStatus))?;
        let form_data: HashMap<&str, &str> = [
            ("package", scoped_package_name.as_str()),
            ("dsrManifestHash", &dsr_manifest_hash),
            ("csrftoken", &csrf_token),
        ]
        .into();
        self.client
            .post(format!(
                "{}/package/%40{}%2F{}/delete",
                NPMJS_COM_URL, self.config.npm_scope, package
            ))
            .form(&form_data)
            .send()
            .await
            .map(|_| Ok(0))
            .map_err(|_| ApiError::NpmError(NpmError::FailedToDeletePackage))?
    }

    /// Add a user to an npm team
    pub async fn add_user_to_team(&self, params: &str, module: &str) -> Result<i32, ApiError> {
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let params: AddRemoveUserToFromTeamParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let user = self.validate_npm_username(&params.user)?;
        let team = self.validate_npm_team_name(&params.team)?;

        info!(
            "Adding user [{}] to team [{}] on behalf of [{module}]",
            user, team
        );

        let csrf_token = self
            .get_csrftoken_from_cookies()
            .map_err(|_| ApiError::NpmError(NpmError::WrongClientStatus))?;
        let body = AddUserToTeamPayload {
            csrftoken: &csrf_token,
            user,
        };
        let body = serde_json::to_string(&body)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToAddUserToTeam))?;
        self.client
            .post(format!(
                "{}/settings/{}/teams/team/{}/users",
                NPMJS_COM_URL, self.config.npm_scope, team
            ))
            .header("Content-Type", "text/plain;charset=UTF-8")
            .body(body)
            .send()
            .await
            .map(|_| Ok(0))
            .map_err(|_| ApiError::NpmError(NpmError::FailedToAddUserToTeam))?
    }

    /// Remove a user from an npm team
    pub async fn remove_user_from_team(&self, params: &str, module: &str) -> Result<i32, ApiError> {
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let params: AddRemoveUserToFromTeamParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let user = self.validate_npm_username(&params.user)?;
        let team = self.validate_npm_team_name(&params.team)?;

        info!("Removing [{user}] from [{team}] on behalf of [{module}]");

        let csrf_token = self
            .get_csrftoken_from_cookies()
            .map_err(|_| ApiError::NpmError(NpmError::WrongClientStatus))?;
        let body = RemoveUserPayload {
            csrftoken: &csrf_token,
        };
        let body = serde_json::to_string(&body)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRemoveUserFromTeam))?;
        self.client
            .post(format!(
                "{}/settings/{}/teams/team/{}/users/{}/delete",
                NPMJS_COM_URL, self.config.npm_scope, team, user
            ))
            .header("Content-Type", "text/plain;charset=UTF-8")
            .body(body)
            .send()
            .await
            .map(|_| Ok(0))
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRemoveUserFromTeam))?
    }

    /// Remove a user from the npm organization
    pub async fn remove_user_from_organization(
        &self,
        user: &str,
        module: &str,
    ) -> Result<i32, ApiError> {
        let user = self.validate_npm_username(user)?;
        info!("Removing user [{user}] from npm organization on behalf of [{module}]");

        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let csrf_token = self
            .get_csrftoken_from_cookies()
            .map_err(|_| ApiError::NpmError(NpmError::WrongClientStatus))?;
        let payload = RemoveUserPayload {
            csrftoken: &csrf_token,
        };

        self.client
            .post(format!(
                "{}/settings/{}/members/{}/delete",
                NPMJS_COM_URL, self.config.npm_scope, user
            ))
            .json(&payload)
            .send()
            .await
            .map(|_| Ok(0))
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRemoveUserFromOrg))?
    }

    /// Invite a user to the npm organization.
    ///
    /// If `team` is specified then the user is added to that team upon accepting the invite.
    /// If `team` is `None`, then the user is added to the default "developers" team.
    pub async fn invite_user_to_organization(
        &self,
        params: &str,
        module: &str,
    ) -> Result<i32, ApiError> {
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let params: InviteUserToOrganizationParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let user = self.validate_npm_username(&params.user)?;

        // See https://docs.npmjs.com/about-developers-team
        // We use "developers" as default since all organizations have a "developers" team
        let team = params.team.unwrap_or("developers".to_string());
        let team = self.validate_npm_team_name(&team)?;

        info!(
            "Inviting user [{}] to npm organization and team [{}] on behalf of [{module}]",
            user, team
        );

        let body = InviteUserToOrganizationPayload {
            csrftoken: &self
                .get_csrftoken_from_cookies()
                .map_err(|_| ApiError::NpmError(NpmError::WrongClientStatus))?,
            team,
            user: [("name", user)].into(),
        };
        let body = serde_json::to_string(&body)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?;

        self.client
            .post(format!(
                "{}/settings/{}/invite/create",
                NPMJS_COM_URL, self.config.npm_scope
            ))
            .header("Content-Type", "text/plain;charset=UTF-8")
            .body(body)
            .send()
            .await
            .map(|_| Ok(0))
            .map_err(|_| ApiError::NpmError(NpmError::FailedToInviteUserToOrg))?
    }

    /// Return all users in the npm organization
    pub async fn get_org_user_list(&self, _: &str, module: &str) -> Result<String, ApiError> {
        info!("Listing all members of npm organization on behalf of [{module}]");
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let response = self
            .client
            .get(format!(
                "{}/settings/{}/members",
                NPMJS_COM_URL, self.config.npm_scope
            ))
            .header("X-Spiferack", "1")
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?
            .json::<Value>()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?;

        // Get total number of users that we will need to retrieve
        let total_users: u64 = serde_json::from_value(
            response
                .get("list")
                .ok_or(ApiError::NpmError(NpmError::FailedToRetrieveUserList))?
                .get("total")
                .ok_or(ApiError::NpmError(NpmError::FailedToRetrieveUserList))?
                .clone(),
        )
        .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?;

        let users = self
            .get_paginated_data_from_npm_website::<NpmUser>(
                &format!(
                    "{}/settings/{}/members",
                    NPMJS_COM_URL, self.config.npm_scope
                ),
                total_users,
            )
            .await?;

        serde_json::to_string(&users)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))
    }

    /// Retrieve all users in the npm org that do not have 2FA enabled
    pub async fn get_org_users_without_2fa(
        &self,
        _: &str,
        module: &str,
    ) -> Result<String, ApiError> {
        info!("Listing all members of npm organization without 2FA on behalf of [{module}]");
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let response = self
            .client
            .get(format!(
                "{}/settings/{}/members",
                NPMJS_COM_URL, self.config.npm_scope
            ))
            .query(&[("selectedTab", "tfa_disabled")])
            .header("X-Spiferack", "1")
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?
            .json::<Value>()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?;

        // Get total number of users that we will need to retrieve
        let total_users: u64 = serde_json::from_value(
            response
                .get("memberCounts")
                .ok_or(ApiError::NpmError(NpmError::FailedToRetrieveUserList))?
                .get("tfa_disabled")
                .ok_or(ApiError::NpmError(NpmError::FailedToRetrieveUserList))?
                .clone(),
        )
        .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?;

        let users = self
            .get_paginated_data_from_npm_website::<NpmUser>(
                &format!(
                    "{}/settings/{}/members?selectedTab=tfa_disabled",
                    NPMJS_COM_URL, self.config.npm_scope
                ),
                total_users,
            )
            .await?;

        serde_json::to_string(&users)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))
    }

    /// Retrieve a list of packages for which a given team has a given permission
    /// (useful to spot misconfigured package permissions).
    pub async fn list_packages_with_team_permission(
        &self,
        params: &str,
        module: &str,
    ) -> Result<String, ApiError> {
        let params: ListPackagesWithTeamPermissionParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let team = self.validate_npm_team_name(&params.team)?;

        info!(
            "Listing all packages for which team [{}] has [{}] access on behalf of [{module}]",
            team,
            params.permission.to_string()
        );
        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;

        let response: Value = self
            .client
            .get(format!(
                "{}/settings/{}/teams/team/{}/access",
                NPMJS_COM_URL, self.config.npm_scope, team
            ))
            .header("X-Spiferack", "1")
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePackages))?
            .json()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePackages))?;

        // Get total number of packages that we will need to retrieve
        let total_packages: u64 = serde_json::from_value(
            response
                .get("list")
                .ok_or(ApiError::NpmError(NpmError::FailedToRetrievePackages))?
                .get("total")
                .ok_or(ApiError::NpmError(NpmError::FailedToRetrievePackages))?
                .clone(),
        )
        .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePackages))?;

        // Get all packages the team can access and keep only those
        // that have the desired permission (specified in the params)
        let packages = self
            .get_paginated_data_from_npm_website::<NpmPackageWithPermission>(
                &format!(
                    "{}/settings/{}/teams/team/{}/access",
                    NPMJS_COM_URL, self.config.npm_scope, team
                ),
                total_packages,
            )
            .await?
            .iter()
            .filter(|v| v.permission == params.permission)
            .cloned()
            .collect::<Vec<NpmPackageWithPermission>>();

        serde_json::to_string(&packages)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePackages))
    }

    /// Utility function that retrieves paginated data from a certain URL under the npm website.
    /// The function keeps querying more pages until it has retrieved the target number of items.
    async fn get_paginated_data_from_npm_website<T: Paginable>(
        &self,
        url: &str,
        target_num_items: u64,
    ) -> Result<Vec<T>, ApiError> {
        // Get the first page, which contains the first items (max 10)
        let response = self
            .client
            .get(url)
            .header("X-Spiferack", "1")
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePaginatedData))?
            .json::<Value>()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePaginatedData))?;

        let mut all_items = T::from_paginated_response(&response)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePaginatedData))?;

        if target_num_items <= 10 {
            // We should have already got 10 items from the first page
            if all_items.len() == target_num_items as usize {
                // OK we got them all and we are done
                return Ok(all_items);
            }
            // We _should_ have got all the items but something went wrong. Not good.
            return Err(ApiError::NpmError(NpmError::FailedToRetrievePaginatedData));
        }

        // If we are here, then there are more than 10 items, so we make more requests
        let mut page_num = 1; // pages start from 0

        loop {
            let response = self
                .client
                .get(url)
                .header("X-Spiferack", "1") // to get JSON instead of HTML
                .query(&[("page", page_num.to_string().as_str()), ("perPage", "10")])
                .send()
                .await
                .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePaginatedData))?
                .json::<Value>()
                .await
                .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePaginatedData))?;

            all_items.extend(
                T::from_paginated_response(&response)
                    .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePaginatedData))?,
            );

            if all_items.len() == target_num_items as usize {
                // We got all the items
                return Ok(all_items);
            }

            // There are more items
            page_num += 1;
        }
    }

    /// Return a JSON-encoded struct that contains details about a granular token
    pub async fn get_token_details(&self, params: &str, module: &str) -> Result<String, ApiError> {
        let params: GetTokenDetailsParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let token_id = self.validate_token_id(&params.token_id)?;

        info!("Retrieving details for token [{token_id}] on behalf of [{module}]");

        self.login()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::LoginFlowError))?;
        let response = self
            .client
            .get(format!(
                "{}/settings/{}/tokens/granular-access-tokens/{}",
                NPMJS_COM_URL, self.config.username, token_id
            ))
            .header("X-Spiferack", "1") // to get JSON instead of HTML
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToGetTokenDetails))?;

        // Information on the token is returned in an object under "tokenDetails"
        let token_details = serde_json::from_value::<GranularTokenDetails>(
            response
                .json::<Value>()
                .await
                .map_err(|_| ApiError::NpmError(NpmError::FailedToGetTokenDetails))?
                .get("tokenDetails")
                .ok_or(ApiError::NpmError(NpmError::FailedToGetTokenDetails))?
                .clone(),
        )
        .map_err(|_| ApiError::NpmError(NpmError::FailedToGetTokenDetails))?;

        serde_json::to_string(&token_details)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToGetTokenDetails))
    }
}

impl Paginable for NpmUser {
    fn from_paginated_response(value: &Value) -> Result<Vec<Self>, ()>
    where
        Self: Sized,
    {
        let users = serde_json::from_value::<Vec<Value>>(
            value
                .get("list")
                .ok_or(())?
                .get("objects")
                .ok_or(())?
                .clone(),
        )
        .map_err(|_| ())?;

        let mut all_users: Vec<NpmUser> = vec![];

        for u in users {
            all_users.push(NpmUser {
                username: serde_json::from_value::<String>(
                    u.get("user").ok_or(())?.get("name").ok_or(())?.clone(),
                )
                .map_err(|_| ())?,
                role: NpmUserRole::try_from(u.get("role").ok_or(())?.to_string().replace("\"", ""))
                    .map_err(|_| ())?,
            });
        }

        Ok(all_users)
    }
}

impl Paginable for NpmPackageWithPermission {
    fn from_paginated_response(value: &Value) -> Result<Vec<Self>, ()>
    where
        Self: Sized,
    {
        let packages = serde_json::from_value::<Vec<Value>>(
            value
                .get("list")
                .ok_or(())?
                .get("objects")
                .ok_or(())?
                .clone(),
        )
        .map_err(|_| ())?;

        let mut all_packages: Vec<NpmPackageWithPermission> = vec![];

        for p in packages {
            all_packages.push(NpmPackageWithPermission {
                package_name: serde_json::from_value::<String>(
                    p.get("package").ok_or(())?.get("name").ok_or(())?.clone(),
                )
                .map_err(|_| ())?,
                permission: NpmPackagePermission::try_from(
                    p.get("permissions")
                        .ok_or(())?
                        .to_string()
                        .replace("\"", ""),
                )
                .map_err(|_| ())?,
            });
        }

        Ok(all_packages)
    }
}

#[derive(Serialize, Clone)]
struct NpmPackageWithPermission {
    package_name: String,
    permission: NpmPackagePermission,
}

trait Paginable {
    fn from_paginated_response(value: &Value) -> Result<Vec<Self>, ()>
    where
        Self: Sized;
}
