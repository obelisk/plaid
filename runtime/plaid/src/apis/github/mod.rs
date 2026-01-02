mod actions;
mod code;
mod copilot;
mod deploy_keys;
mod environments;
mod graphql;
mod members;
mod pats;
mod pull_requests;
mod refs;
mod repos;
mod secrets;
mod teams;
mod validators;

use http::{header::USER_AGENT, HeaderMap};
use jsonwebtoken::EncodingKey;
use octocrab::{NoAuth, Octocrab};

use serde::{Deserialize, Serialize};

use std::{collections::HashMap, sync::Arc};

use crate::loader::PlaidModule;

use super::ApiError;

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Authentication {
    /// Do not use any authentication when making requests to the GitHub API. This will
    /// limit you to only public APIs that do not require authentication.
    NoAuth {},
    /// If you provide a token then we will initialize the client using that
    /// method of authentication. This is generally simpler to set up but less
    /// secure and doesn't have access to all the same APIs (for example approving
    /// finegrained or classic access tokens)
    Token { token: String },
    /// If you provide an app then we will initalize the system as a GitHub app
    /// This is more secure but requires more setup and is more prone to specific API
    /// failures if the app has not been granted permissions correctly.
    App {
        app_id: u64,
        installation_id: u64,
        private_key: String,
    },
}

#[derive(Deserialize)]
/// The configuration structure that forms the API module to service
/// requests from running Plaid modules.
pub struct GithubConfig {
    /// The authentication method used when configuring the GitHub API module. More
    /// methods may be added here in the future but one variant of the enum must be defined.
    /// See the Authentication enum structure above for more details.
    authentication: Authentication,
    /// This is a map of GraphQL queries you are allowing rules to execute. These are
    /// manually specified to reduce the risk of abuse by rules as they are very powerful
    /// and hard to reason about in a generic way, especially at runtime.
    graphql_queries: HashMap<String, String>,
}

/// Represents the configured GitHub API
pub struct Github {
    /// Configuration for Plaid's GitHub API
    config: GithubConfig,
    /// Client to make requests with
    client: Octocrab,
    /// Validators used to check parameters passed by modules
    validators: HashMap<&'static str, regex::Regex>,
}

/// All the errors that can be encountered while executing GitHub calls
#[derive(Debug)]
pub enum GitHubError {
    GraphQLUnserializable,
    GraphQLQueryUnknown(String),
    GraphQLInvalidCharacters(String),
    UnexpectedStatusCode(u16),
    GraphQLRequestError(String),
    ClientError(octocrab::Error),
    InvalidInput(String),
    BadResponse,
}

impl Github {
    pub fn new(config: GithubConfig) -> Self {
        let client = build_github_client(&config.authentication);

        // Create all the validators and compile all the regexes. If the module contains
        // any invalid regexes it will panic.
        let validators = validators::create_validators();

        Self {
            config,
            client,
            validators,
        }
    }

    /// Make a generic get request to the GitHub API using the GitHub app library. This exists
    /// to help facilitate the conversion from a token usage to GitHub app. It also means that
    /// extra parsing can be avoided since we need to re-serialize anyway to pass back to the rules
    /// (at least currently).
    async fn make_generic_get_request(
        &self,
        uri: String,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        self.make_get_request_with_headers(uri, None, module).await
    }

    /// Make a GET request with custom headers to the GitHub API using the GitHub app library.
    /// Note - This function does not do any validation on the provided headers. That's because
    /// it's not exposed to the rules but only callable from within the runtime itself. Therefore
    /// we assume that all necessary validation has already been performed by the calling function.
    async fn make_get_request_with_headers(
        &self,
        uri: String,
        headers: Option<HeaderMap>,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        // We log the header names we are passing but not the values, in case they are sensitive.
        info!(
            "Making a get request to {uri} on behalf of {module}. Provided headers: {:?}",
            match headers {
                Some(ref headers) => headers
                    .keys()
                    .map(|v| v.as_str())
                    .collect::<Vec<&str>>()
                    .join(", "),
                None => "None".to_string(),
            }
        );

        let request = self.client._get_with_headers(uri, headers).await;

        match request {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = self.client.body_to_string(r).await.map_err(|e| {
                    ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string()))
                });
                Ok((status, body))
            }
            Err(e) => Err(ApiError::GitHubError(GitHubError::ClientError(e))),
        }
    }

    /// Make a generic post request to the GitHub API using the GitHub app library. This exists
    /// to help facilitate the conversion from a token usage to GitHub app. It also means that
    /// extra parsing can be avoided since we need to re-serialize anyway to pass back to the rules
    /// (at least currently).
    async fn make_generic_post_request<T: Serialize>(
        &self,
        uri: String,
        body: T,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        info!("Making a post request to {uri} on behalf of {module}");

        let request = self.client._post(uri, Some(&body)).await;

        match request {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = self.client.body_to_string(r).await.map_err(|e| {
                    ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string()))
                });
                Ok((status, body))
            }
            Err(e) => Err(ApiError::GitHubError(GitHubError::ClientError(e))),
        }
    }

    /// Make a generic put request to the GitHub API using the GitHub app library. This exists
    /// to help facilitate the conversion from a token usage to GitHub app. It also means that
    /// extra parsing can be avoided since we need to re-serialize anyway to pass back to the rules
    /// (at least currently).
    async fn make_generic_put_request<T: Serialize>(
        &self,
        uri: String,
        body: Option<&T>,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        info!("Making a put request to {uri} on behalf of {module}");

        let request = self.client._put(uri, body).await;

        match request {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = self.client.body_to_string(r).await.map_err(|e| {
                    ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string()))
                });
                Ok((status, body))
            }
            Err(e) => Err(ApiError::GitHubError(GitHubError::ClientError(e))),
        }
    }

    /// Make a generic delete request to the GitHub API using the GitHub app library. This exists
    /// to help facilitate the conversion from a token usage to GitHub app. It also means that
    /// extra parsing can be avoided since we need to re-serialize anyway to pass back to the rules
    /// (at least currently).
    async fn make_generic_delete_request<T: Serialize>(
        &self,
        uri: String,
        body: Option<&T>,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        info!("Making a delete request to {uri} on behalf of {module}");

        let request = self.client._delete(uri, body).await;

        match request {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = self.client.body_to_string(r).await.map_err(|e| {
                    ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string()))
                });
                Ok((status, body))
            }
            Err(e) => Err(ApiError::GitHubError(GitHubError::ClientError(e))),
        }
    }
}

/// Builds an instance of a Github API client
pub fn build_github_client(authentication: &Authentication) -> Octocrab {
    let client_builder = match authentication {
        Authentication::NoAuth {} => {
            info!("Configuring GitHub client without authentication");
            Octocrab::builder().with_auth(NoAuth {})
        }
        Authentication::Token { token } => {
            info!("Configuring GitHub client with GitHub PAT");
            Octocrab::builder().personal_token(token.clone())
        }
        Authentication::App {
            app_id,
            private_key,
            ..
        } => {
            info!("Configuring GitHub client with GitHub App");
            let encoding_key = EncodingKey::from_rsa_pem(private_key.as_bytes())
                .expect("Failed to create encoding key from private key for GitHub API");

            Octocrab::builder().app((*app_id).into(), encoding_key)
        }
    }
    .add_header(
        USER_AGENT,
        format!("Rust/Plaid{}", env!("CARGO_PKG_VERSION")),
    );

    let mut client = client_builder
        .build()
        .expect("Failed to create GitHub client");

    if let Authentication::App {
        installation_id, ..
    } = authentication
    {
        client = client.installation((*installation_id).into());
    }

    client
}
