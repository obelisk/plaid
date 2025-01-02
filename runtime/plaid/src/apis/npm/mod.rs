mod hashes;
mod validators;

pub mod npm_cli_client;
pub mod npm_web_client;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use plaid_stl::npm::shared_structs::NpmError;
use regex::Regex;
use reqwest::Client;
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde::{Deserialize, Serialize};

/// Object for interacting with npm
pub struct Npm {
    /// Config for the npm API
    config: NpmConfig,
    /// A client to make requests with
    client: Client,
    /// Store for cookies, which are used to persist state across invocations
    cookie_jar: Arc<CookieStoreMutex>,
    /// Validators which are used to check values passed by invoking modules
    validators: HashMap<&'static str, regex::Regex>,
    /// The timestamp at which the last request was executed. This is used to determine
    /// if cookies in the cookie jar can be reused or should be considered stale. If cookies
    /// are stale, then a new log in flow will be executed.
    /// This mechanism is not perfect but achieves a good balance between random failures due to
    /// expired sessions and having to perform a fresh log in every single time (possibly, multiple
    /// times for a given request)
    timestamp_last_request: Mutex<Option<u32>>,
}

impl Npm {
    pub fn new(config: NpmConfig) -> Result<Self, NpmError> {
        let cookie_jar = CookieStore::new(None);
        let cookie_jar = CookieStoreMutex::new(cookie_jar);
        let cookie_jar = Arc::new(cookie_jar);
        let client = Client::builder()
            .cookie_provider(Arc::clone(&cookie_jar))
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
            timestamp_last_request: Mutex::new(None),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// Configuration for the npm API
pub struct NpmConfig {
    /// Username for the npm account
    pub username: String,
    /// Password for the npm account
    pub password: String,
    /// Secret for the TOTP-based 2FA on the npm account. If the account does not have 2FA, then
    /// the login cannot be automated. This is because when 2FA is not enabled, npmjs.com sends
    /// a one-time code to the registered email address, so plaid cannot fetch it.
    pub otp_secret: String,
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
        otp_secret: String,
        automation_token: String,
        npm_scope: String,
        user_agent: String,
    ) -> Result<Self, NpmError> {
        // Check the OTP secret looks OK: it should be 32 alphanumerical characters
        // Safe unwrap: hardcoded regex
        let otp_regex = Regex::new(r"^[a-zA-Z0-9]{32}$").unwrap();
        if !otp_regex.is_match(&otp_secret) {
            return Err(NpmError::WrongConfig(
                "Wrong format for OTP secret".to_string(),
            ));
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
