use reqwest::{cookie::CookieStore, Response};
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use totp_rs::{Algorithm, Secret, TOTP};
use url::Url;

use crate::{apis::ApiError, loader::PlaidModule};

use plaid_stl::npm::shared_structs::*;

use super::Npm;

const NPMJS_COM_URL: &str = "https://www.npmjs.com";

#[derive(Serialize)]
/// Payload sent to npm website to change a team's permissions over a package
struct PermissionChangePayload<'a> {
    /// CSRF token to validate the request
    csrftoken: &'a str,
    /// The package we are changing permissions for
    package: &'a str,
    /// The permission we are setting
    permissions: &'a str,
}

#[derive(Serialize)]
/// Payload sent to npm website to generate a new granular token
struct GenerateGranularTokenPayload<'a> {
    /// IPs that can use the token
    #[serde(rename = "allowedIPRanges")]
    allowed_ip_ranges: Vec<String>,
    /// CSRF token to validate the request
    csrftoken: &'a str,
    /// After how many days the token will expire
    #[serde(rename = "expirationDays")]
    expiration_days: &'a str,
    /// Permissions that the token has on the selected npm org
    #[serde(rename = "orgsPermission")]
    orgs_permission: &'a str,
    /// Permissions that the token has on the selected packages
    #[serde(rename = "packagesAndScopesPermission")]
    packages_and_scopes_permission: &'a str,
    /// Which organizations the token has permissions on
    #[serde(rename = "selectedOrgs")]
    selected_orgs: Vec<String>,
    /// Which packages the token has permissions on
    #[serde(rename = "selectedPackages")]
    selected_packages: Vec<String>,
    /// Which packages and scopes the token has permissions on
    #[serde(rename = "selectedPackagesAndScopes")]
    selected_packages_and_scopes: &'a str,
    /// Which scopes the token has permissions on
    #[serde(rename = "selectedScopes")]
    selected_scopes: Vec<String>,
    /// A description for the token
    #[serde(rename = "tokenDescription")]
    token_description: &'a str,
    /// The token's name. This must be unique.
    #[serde(rename = "tokenName")]
    token_name: &'a str,
}

/// Payload sent to npm website to invite a user into the configured npm organization
#[derive(Serialize)]
struct InviteUserToOrganizationPayload<'a> {
    /// CSRF token to validate the request
    csrftoken: &'a str,
    /// The team the user will be added to, upon accepting the invite
    team: &'a str,
    /// Map that contains "name": <user name>
    user: HashMap<&'a str, &'a str>,
}

/// Payload sent to npm website to add a user to a given team in the npm org
#[derive(Serialize)]
struct AddUserToTeamPayload<'a> {
    /// CSRF token to validate the request
    csrftoken: &'a str,
    /// Username
    user: &'a str,
}

/// Payload sent to npm website to remove a user from a team or from the configured npm org.
#[derive(Serialize)]
struct RemoveUserPayload<'a> {
    /// CSRF token to validate the request
    csrftoken: &'a str,
}

/// Payload sent to npm website to delete an access token.
#[derive(Serialize)]
struct DeleteTokenPayload<'a> {
    /// CSRF token to validate the request
    csrftoken: &'a str,
    /// The ID of the token to be deleted (only one)
    tokens: &'a str,
}

/// Groups an npm package name with a permission (read or write).
#[derive(Serialize, Clone)]
struct NpmPackageWithPermission {
    /// Name of the package
    package_name: String,
    /// Permission on the package (read or write)
    permission: NpmPackagePermission,
}

/// Takes a response with an unexpected status code and serializes it to
/// a String that contains all the context that a rule might need to handle
/// this unexpected response.
fn handle_bad_response(response: &Response) -> String {
    let error = bad_response_to_error(response);
    return RuntimeReturnValue::serialize_from_err(error);
}

/// Takes a response with an unexpected status code and turns it into
/// an appropriate NpmError
fn bad_response_to_error(response: &Response) -> NpmError {
    // If we are being throttled, i.e., if npm returns 429, see if there is a
    // Retry-After header. If so, we pass it back to the calling module, which
    // is then responsible for taking appropriate action.
    // If it's another unexpected status code, then just wrap it in an NpmError
    // and pass it through.
    match response.status().as_u16() {
        429 => {
            let retry_after = response
                .headers()
                .get("Retry-After")
                .map(|r| r.to_str().ok().map(|v| v.parse::<u32>().ok()))
                .flatten()
                .flatten();
            NpmError::ThrottlingError(retry_after)
        }
        status => NpmError::UnexpectedStatusCode(status),
    }
}

impl Npm {
    /// Retrieve the CSRF token from the client's cookie jar
    fn get_csrftoken_from_cookies(&self) -> Result<String, NpmError> {
        let cookies: Vec<String> = self
            .cookie_jar
            .as_ref()
            // safe unwrap: data is hardcoded
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
        // Decide if we should clear the cookies and start from a fresh state.
        // The strategy is as follows: if more than 2 minutes have passed since the last request,
        // then we clear the cookies and perform a new login from scratch. This allows us to avoid
        // repeated logins for actions that belong to the same "flow" and are executed rapidly one
        // after the other, while making sure that our requests do not fail because some cookies have
        // become stale. In fact, it is quite hard to understand if the request "worked" or not, since
        // they all return 200 and, in many cases, parsing the returned HTML would be necessary.
        {
            // TODO Double check this unwrap
            let mut timestamp_last_request = self.timestamp_last_request.lock().unwrap();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs() as u32;
            if let Some(last_req) = *timestamp_last_request {
                if now - last_req >= 120 {
                    // TODO double check this unwrap
                    self.cookie_jar.lock().unwrap().clear();
                    *timestamp_last_request = Some(now);
                }
            } else {
                // The timestamp was None, so we set it
                *timestamp_last_request = Some(now);
            }
        }

        let response = self
            .client
            .get(format!("{}/login", NPMJS_COM_URL))
            .send()
            .await
            .map_err(|_| NpmError::LoginFlowError)?;
        if response.status().as_u16() != 200 {
            return Err(bad_response_to_error(&response));
        }

        // Get the CSRF token (which is in a cookie) because we need to send it back later on
        let cs_cookie = match response.cookies().find(|c| c.name() == "cs") {
            None => return Ok(()), // we do not get this cookie if we are _already_ logged in
            Some(c) => c.value().to_string(),
        };

        // Login step 1: send the username and password, together with the CSRF token
        let response = self
            .client
            .post(format!("{}/login", NPMJS_COM_URL))
            .form(&[
                ("username", &self.config.username),
                ("password", &self.config.password),
                ("csrftoken", &cs_cookie),
            ])
            .send()
            .await
            .map_err(|_| NpmError::LoginFlowError)?;
        if response.status().as_u16() != 200 {
            return Err(bad_response_to_error(&response));
        }

        // Login step 2: 2FA flow
        let otp_token = TOTP::new(
            Algorithm::SHA1,
            6,
            1,
            30,
            Secret::Encoded(self.config.otp_secret.clone())
                .to_bytes()
                .map_err(|_| NpmError::LoginFlowError)?,
        )
        .map_err(|_| NpmError::LoginFlowError)?
        .generate_current()
        .map_err(|_| NpmError::LoginFlowError)?;

        // Send the TOTP code to a well-known URL
        let response = self
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
            .map_err(|_| NpmError::LoginFlowError)?;
        if response.status().as_u16() != 200 {
            return Err(bad_response_to_error(&response));
        }
        Ok(())
    }

    /// Set a team's permissions over a package
    pub async fn set_team_permission_on_package(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
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
        let response = self
            .client
            .post(format!(
                "{}/settings/{}/teams/team/{}/access",
                NPMJS_COM_URL, self.config.npm_scope, team
            ))
            .json(&payload)
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::PermissionChangeError))?;
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
        Ok(RuntimeReturnValue::serialize_empty_ok())
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
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
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
            expiration_days: &specs.expiration_days.unwrap_or(30).to_string(),
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
            .map_err(|_| {
                ApiError::NpmError(NpmError::TokenGenerationError(
                    "error while calling npm".to_string(),
                ))
            })?;
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }

        // The new token is in the JSON response, under the key "newToken"
        let token = response
            .json::<Value>()
            .await
            .map_err(|_| {
                ApiError::NpmError(NpmError::TokenGenerationError(
                    "error while retrieving JSON from npm response".to_string(),
                ))
            })?
            .get("newToken")
            .ok_or(ApiError::NpmError(NpmError::TokenGenerationError(
                "error while reading token from npm JSON response".to_string(),
            )))?
            .as_str()
            .ok_or(ApiError::NpmError(NpmError::TokenGenerationError(
                "error while reading token from npm JSON response".to_string(),
            )))?
            .to_string();
        Ok(RuntimeReturnValue::serialize_from_str(&token))
    }

    /// Delete a granular token from the npm website
    pub async fn delete_granular_token(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
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
        let response = self
            .client
            .post(format!(
                "{}/settings/{}/tokens/delete",
                NPMJS_COM_URL, self.config.username
            ))
            .json(&payload)
            .header("X-Spiferack", "1") // to get JSON instead of HTML
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::TokenDeletionError))?;
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
        Ok(RuntimeReturnValue::serialize_empty_ok())
    }

    /// Retrieve a list of granular tokens for the account whose credentials have been configured for this client.
    ///
    /// Note: only granular tokens are returned. Other types of tokens (publish, automation, etc.) are filtered out.
    pub async fn list_granular_tokens(
        &self,
        _: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        info!("Listing npm granular tokens on behalf of [{module}]");
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
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
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
        let response = response
            .json::<Value>()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToListGranularTokens))?;

        // Get total number of tokens that we will need to retrieve
        let total_tokens: u64 = serde_json::from_value(
            response
                .get("list")
                .ok_or(ApiError::NpmError(NpmError::FailedToListGranularTokens))?
                .get("total")
                .ok_or(ApiError::NpmError(NpmError::FailedToListGranularTokens))?
                .clone(),
        )
        .map_err(|_| ApiError::NpmError(NpmError::FailedToListGranularTokens))?;

        let tokens = match self
            .get_paginated_data_from_npm_website::<NpmToken>(
                &format!("{}/settings/{}/tokens", NPMJS_COM_URL, self.config.username),
                total_tokens,
            )
            .await
        {
            Ok(tokens) => tokens,
            Err(e) => return Ok(RuntimeReturnValue::serialize_from_err(e)),
        };

        // Filter results and keep only granular tokens
        let tokens: Vec<NpmToken> = tokens
            .iter()
            .filter(|v| v.token_type == Some("granular".to_string()))
            .cloned()
            .collect();

        let tokens = serde_json::to_string(&tokens)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToListGranularTokens))?;
        Ok(RuntimeReturnValue::serialize_from_str(&tokens))
    }

    /// Delete a package from the npm registry.
    ///
    /// Note: The package name should be unscoped. If you are trying to delete
    /// @scope/package_name, then you should pass only "package_name". The scope is
    /// preconfigured in the client and will be added automatically.
    pub async fn delete_package(
        &self,
        package: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let package = self.validate_npm_package_name(package)?;
        info!("Deleting npm package [{package}] on behalf of [{module}]");
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
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
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
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
        let response = self
            .client
            .post(format!(
                "{}/package/%40{}%2F{}/delete",
                NPMJS_COM_URL, self.config.npm_scope, package
            ))
            .form(&form_data)
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToDeletePackage))?;
        if response.status().as_u16() != 302 {
            return Err(ApiError::NpmError(NpmError::UnexpectedStatusCode(
                response.status().as_u16(),
            )));
        }
        Ok(RuntimeReturnValue::serialize_empty_ok())
    }

    /// Add a user to an npm team
    pub async fn add_user_to_team(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
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
        let response = self
            .client
            .post(format!(
                "{}/settings/{}/teams/team/{}/users",
                NPMJS_COM_URL, self.config.npm_scope, team
            ))
            .header("Content-Type", "text/plain;charset=UTF-8")
            .body(body)
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToAddUserToTeam))?;
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
        Ok(RuntimeReturnValue::serialize_empty_ok())
    }

    /// Remove a user from an npm team
    pub async fn remove_user_from_team(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
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
        let response = self
            .client
            .post(format!(
                "{}/settings/{}/teams/team/{}/users/{}/delete",
                NPMJS_COM_URL, self.config.npm_scope, team, user
            ))
            .header("Content-Type", "text/plain;charset=UTF-8")
            .header("X-Spiferack", "1")
            .body(body)
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRemoveUserFromTeam))?;
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
        Ok(RuntimeReturnValue::serialize_empty_ok())
    }

    /// Remove a user from the npm organization
    pub async fn remove_user_from_organization(
        &self,
        user: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let user = self.validate_npm_username(user)?;
        info!("Removing user [{user}] from npm organization on behalf of [{module}]");

        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
        let csrf_token = self
            .get_csrftoken_from_cookies()
            .map_err(|_| ApiError::NpmError(NpmError::WrongClientStatus))?;
        let payload = RemoveUserPayload {
            csrftoken: &csrf_token,
        };

        let response = self
            .client
            .post(format!(
                "{}/settings/{}/members/{}/delete",
                NPMJS_COM_URL, self.config.npm_scope, user
            ))
            .header("X-Spiferack", "1")
            .json(&payload)
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRemoveUserFromOrg))?;
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
        Ok(RuntimeReturnValue::serialize_empty_ok())
    }

    /// Invite a user to the npm organization.
    ///
    /// If `team` is specified then the user is added to that team upon accepting the invite.
    /// If `team` is `None`, then the user is added to the default "developers" team.
    pub async fn invite_user_to_organization(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
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

        let response = self
            .client
            .post(format!(
                "{}/settings/{}/invite/create",
                NPMJS_COM_URL, self.config.npm_scope
            ))
            .header("Content-Type", "text/plain;charset=UTF-8")
            .header("X-Spiferack", "1")
            .body(body)
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToInviteUserToOrg))?;
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
        Ok(RuntimeReturnValue::serialize_empty_ok())
    }

    /// Return all users in the npm organization
    pub async fn get_org_user_list(
        &self,
        _: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        info!("Listing all members of npm organization on behalf of [{module}]");
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
        let response = self
            .client
            .get(format!(
                "{}/settings/{}/members",
                NPMJS_COM_URL, self.config.npm_scope
            ))
            .header("X-Spiferack", "1")
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?;
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
        let response = response
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

        let users = match self
            .get_paginated_data_from_npm_website::<NpmUser>(
                &format!(
                    "{}/settings/{}/members",
                    NPMJS_COM_URL, self.config.npm_scope
                ),
                total_users,
            )
            .await
        {
            Ok(users) => users,
            Err(e) => return Ok(RuntimeReturnValue::serialize_from_err(e)),
        };

        let users = serde_json::to_string(&users)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?;
        Ok(RuntimeReturnValue::serialize_from_str(&users))
    }

    /// Retrieve all users in the npm org that do not have 2FA enabled
    pub async fn get_org_users_without_2fa(
        &self,
        _: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        info!("Listing all members of npm organization without 2FA on behalf of [{module}]");
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
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
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?;
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
        let response = response
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

        let users = match self
            .get_paginated_data_from_npm_website::<NpmUser>(
                &format!(
                    "{}/settings/{}/members?selectedTab=tfa_disabled",
                    NPMJS_COM_URL, self.config.npm_scope
                ),
                total_users,
            )
            .await
        {
            Ok(users) => users,
            Err(e) => return Ok(RuntimeReturnValue::serialize_from_err(e)),
        };

        let users = serde_json::to_string(&users)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrieveUserList))?;
        Ok(RuntimeReturnValue::serialize_from_str(&users))
    }

    /// Retrieve a list of packages for which a given team has a given permission
    /// (useful to spot misconfigured package permissions).
    pub async fn list_packages_with_team_permission(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let params: ListPackagesWithTeamPermissionParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let team = self.validate_npm_team_name(&params.team)?;

        info!(
            "Listing all packages for which team [{}] has [{}] access on behalf of [{module}]",
            team,
            params.permission.to_string()
        );
        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }

        let response = self
            .client
            .get(format!(
                "{}/settings/{}/teams/team/{}/access",
                NPMJS_COM_URL, self.config.npm_scope, team
            ))
            .header("X-Spiferack", "1")
            .send()
            .await
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePackages))?;
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }
        let response: Value = response
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
        let packages = match self
            .get_paginated_data_from_npm_website::<NpmPackageWithPermission>(
                &format!(
                    "{}/settings/{}/teams/team/{}/access",
                    NPMJS_COM_URL, self.config.npm_scope, team
                ),
                total_packages,
            )
            .await
        {
            Ok(packages) => packages
                .iter()
                .filter(|v| v.permission == params.permission)
                .cloned()
                .collect::<Vec<NpmPackageWithPermission>>(),
            Err(e) => return Ok(RuntimeReturnValue::serialize_from_err(e)),
        };
        let packages = serde_json::to_string(&packages)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToRetrievePackages))?;
        Ok(RuntimeReturnValue::serialize_from_str(&packages))
    }

    /// Utility function that retrieves paginated data from a certain URL under the npm website.
    /// The function keeps querying more pages until it has retrieved the target number of items.
    async fn get_paginated_data_from_npm_website<T: Paginable>(
        &self,
        url: &str,
        target_num_items: u64,
    ) -> Result<Vec<T>, NpmError> {
        // Get the first page, which contains the first items (max 10)
        let response = self
            .client
            .get(url)
            .header("X-Spiferack", "1")
            .send()
            .await
            .map_err(|_| NpmError::FailedToRetrievePaginatedData)?;
        if response.status().as_u16() != 200 {
            return Err(bad_response_to_error(&response));
        }
        let response = response
            .json::<Value>()
            .await
            .map_err(|_| NpmError::FailedToRetrievePaginatedData)?;

        let mut all_items = T::from_paginated_response(&response)
            .map_err(|_| NpmError::FailedToRetrievePaginatedData)?;

        if target_num_items <= 10 {
            // We should have already got 10 items from the first page
            if all_items.len() == target_num_items as usize {
                // OK we got them all and we are done
                return Ok(all_items);
            }
            // We _should_ have got all the items but something went wrong. Not good.
            return Err(NpmError::FailedToRetrievePaginatedData);
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
                .map_err(|_| NpmError::FailedToRetrievePaginatedData)?;
            if response.status().as_u16() != 200 {
                return Err(bad_response_to_error(&response));
            }
            let response = response
                .json::<Value>()
                .await
                .map_err(|_| NpmError::FailedToRetrievePaginatedData)?;

            all_items.extend(
                T::from_paginated_response(&response)
                    .map_err(|_| NpmError::FailedToRetrievePaginatedData)?,
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
    pub async fn get_token_details(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let params: GetTokenDetailsParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let token_id = self.validate_token_id(&params.token_id)?;

        info!("Retrieving details for token [{token_id}] on behalf of [{module}]");

        if let Err(e) = self.login().await {
            return Ok(RuntimeReturnValue::serialize_from_err(e));
        }
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
        if response.status().as_u16() != 200 {
            return Ok(handle_bad_response(&response));
        }

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

        let details = serde_json::to_string(&token_details)
            .map_err(|_| ApiError::NpmError(NpmError::FailedToGetTokenDetails))?;
        Ok(RuntimeReturnValue::serialize_from_str(&details))
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

impl Paginable for NpmToken {
    fn from_paginated_response(value: &Value) -> Result<Vec<Self>, ()>
    where
        Self: Sized,
    {
        let tokens = serde_json::from_value::<Vec<NpmToken>>(
            value
                .get("list")
                .ok_or(())?
                .get("objects")
                .ok_or(())?
                .clone(),
        )
        .map_err(|_| ())?;

        Ok(tokens)
    }
}

/// Trait for objects that can be constructed from a paginated response provided by the npm website.
trait Paginable {
    fn from_paginated_response(value: &Value) -> Result<Vec<Self>, ()>
    where
        Self: Sized;
}
