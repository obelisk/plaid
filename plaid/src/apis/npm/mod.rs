mod hashes;
mod validators;

pub mod npm_cli_client;
pub mod npm_web_client;

use std::{collections::HashMap, sync::Arc};

use regex::Regex;
use reqwest::{cookie::Jar, Client};
use serde::{Deserialize, Serialize};

/// Client for interacting with npm that groups a client for CLI operations
/// and one for web operations
pub struct Npm {
    config: NpmConfig,
    client: Client,
    cookie_jar: Arc<Jar>,
    validators: HashMap<&'static str, regex::Regex>,
}

impl Npm {
    pub fn new(config: NpmConfig) -> Result<Self, NpmError> {
        let cookie_jar = Arc::new(Jar::default());
        let client = Client::builder()
            .cookie_provider(cookie_jar.clone())
            .build()
            .map_err(|_| NpmError::GenericError)?;

        // Create all the validators and compile all the regexes. If the module contains
        // any invalid regexes it will panic.
        let validators = validators::create_validators();

        Ok(Self {
            config,
            client,
            cookie_jar,
            validators,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// Credentials and secrets for interacting with npm
pub struct NpmConfig {
    /// Username for the npm account
    pub username: String,
    /// Password for the npm account
    pub password: String,
    /// Secret for the TOTP-based 2FA on the npm account
    pub otp_secret: Option<String>,
    /// Automation (not publish!) token for the npm account. This is a type of token that can
    /// be created through the npm website and allows this user to publish packages without
    /// having to complete the 2FA flow. It is used in the CLI client, for publishing a new package
    pub automation_token: String,
    /// The scope for npm packages we are managing. This corresponds to the name of the organization
    pub npm_scope: String,
    /// The content of the user-agent header to pass when making a request via the CLI client.
    /// Useful to link logs together
    pub user_agent: String,
}

impl NpmConfig {
    pub fn new(
        username: String,
        password: String,
        otp_secret: Option<String>,
        automation_token: String,
        npm_scope: String,
        user_agent: String,
    ) -> Result<Self, NpmError> {
        // Check the OTP secret looks OK: it should be 32 alphanumerical characters
        if let Some(ref otp_secret) = otp_secret {
            // Safe unwrap: hardcoded regex
            let otp_regex = Regex::new(r"^[a-zA-Z0-9]{32}$").unwrap();
            if !otp_regex.is_match(otp_secret) {
                return Err(NpmError::WrongConfig(
                    "Wrong format for OTP secret".to_string(),
                ));
            }
        }

        // Check the automation_token looks OK: it should be "npm_" followed by 36 alphanum characters
        let automation_token_regex = Regex::new(r"^npm_[a-zA-Z0-9]{36}$").unwrap();
        if !automation_token_regex.is_match(&automation_token) {
            return Err(NpmError::WrongConfig(
                "Wrong format for automation token".to_string(),
            ));
        }

        Ok(Self {
            username,
            password,
            otp_secret,
            automation_token,
            npm_scope,
            user_agent,
        })
    }
}

#[derive(Debug)]
pub enum NpmError {
    InvalidInput(String),
    RegistryUploadError,
    FailedToGenerateArchive,
    PermissionChangeError,
    GenericError,
    LoginFlowError,
    WrongClientStatus,
    TokenGenerationError,
    TokenDeletionError,
    WrongConfig(String),
    FailedToListGranularTokens,
    FailedToDeletePackage,
    FailedToAddUserToTeam,
    FailedToRemoveUserFromTeam,
    FailedToRemoveUserFromOrg,
    FailedToInviteUserToOrg,
    FailedToRetrieveUserList,
    FailedToRetrieveUsersWithout2FA,
    FailedToConvertToNpmUser,
    FailedToGetCsrfTokenFromCookies,
    FailedToRetrievePaginatedData,
    FailedToRetrievePackages,
    FailedToGetTokenDetails,
}
