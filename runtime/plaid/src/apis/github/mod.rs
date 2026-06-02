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

use std::{collections::HashMap, fmt::Display, sync::Arc};

use crate::loader::PlaidModule;

use super::ApiError;

#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum Authentication {
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
    /// Do not use any authentication when making requests to the GitHub API. This will
    /// limit you to only public APIs that do not require authentication.
    /// NOTE - THIS MUST BE LAST IN THE ENUM BECAUSE IT ACTS AS A CATCH-ALL.
    NoAuth {},
}

#[derive(Deserialize)]
/// The configuration structure that forms the API module to service
/// requests from running Plaid modules.
pub struct GithubConfig {
    /// The authentication method used when configuring the GitHub API module. More
    /// methods may be added here in the future but one variant of the enum must be defined.
    /// See the Authentication enum structure above for more details.
    /// This is a map from a string identifier to the authentication method, since Plaid supports
    /// multiple GitHub connections in the same runtime.
    authentication: HashMap<String, Authentication>,
    /// This is a map of GraphQL queries you are allowing rules to execute. These are
    /// manually specified to reduce the risk of abuse by rules as they are very powerful
    /// and hard to reason about in a generic way, especially at runtime.
    graphql_queries: HashMap<String, String>,
}

/// Represents the configured GitHub API
pub struct Github {
    /// Configuration for Plaid's GitHub API
    config: GithubConfig,
    /// Clients to make requests with, keyed by their identifier
    clients: HashMap<String, Octocrab>,
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
    pub fn new(config: GithubConfig) -> Result<Self, ApiError> {
        let clients = build_github_clients(&config.authentication)?;

        // Create all the validators and compile all the regexes. If the module contains
        // any invalid regexes it will panic.
        let validators = validators::create_validators();

        Ok(Self {
            config,
            clients,
            validators,
        })
    }

    /// Make a generic get request to the GitHub API using the GitHub app library. This exists
    /// to help facilitate the conversion from a token usage to GitHub app. It also means that
    /// extra parsing can be avoided since we need to re-serialize anyway to pass back to the rules
    /// (at least currently).
    async fn make_generic_get_request(
        &self,
        client_id: impl Display,
        uri: String,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        self.make_get_request_with_headers(client_id, uri, None, module)
            .await
    }

    /// Make a GET request with custom headers to the GitHub API using the GitHub app library.
    /// Note - This function does not do any validation on the provided headers. That's because
    /// it's not exposed to the rules but only callable from within the runtime itself. Therefore
    /// we assume that all necessary validation has already been performed by the calling function.
    async fn make_get_request_with_headers(
        &self,
        client_id: impl Display,
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

        let client = self.clients.get(&client_id.to_string()).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                client_id
            )))
        })?;

        let request = client._get_with_headers(uri, headers).await;

        match request {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = client.body_to_string(r).await.map_err(|e| {
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
        client_id: impl Display,
        uri: String,
        body: T,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        info!("Making a post request to {uri} on behalf of {module}");

        let client = self.clients.get(&client_id.to_string()).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                client_id
            )))
        })?;

        let request = client._post(uri, Some(&body)).await;

        match request {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = client.body_to_string(r).await.map_err(|e| {
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
        client_id: impl Display,
        uri: String,
        body: Option<&T>,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        info!("Making a put request to {uri} on behalf of {module}");

        let client = self.clients.get(&client_id.to_string()).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                client_id
            )))
        })?;

        let request = client._put(uri, body).await;

        match request {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = client.body_to_string(r).await.map_err(|e| {
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
        client_id: impl Display,
        uri: String,
        body: Option<&T>,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        info!("Making a delete request to {uri} on behalf of {module}");

        let client = self.clients.get(&client_id.to_string()).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                client_id
            )))
        })?;

        let request = client._delete(uri, body).await;

        match request {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = client.body_to_string(r).await.map_err(|e| {
                    ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string()))
                });
                Ok((status, body))
            }
            Err(e) => Err(ApiError::GitHubError(GitHubError::ClientError(e))),
        }
    }
}

/// Builds an instance of a Github API client
pub fn build_github_clients(
    authentication: &HashMap<String, Authentication>,
) -> Result<HashMap<String, Octocrab>, ApiError> {
    if authentication.is_empty() {
        return Err(ApiError::GitHubError(GitHubError::InvalidInput(
            "At least one GitHub client must be configured".to_string(),
        )));
    }

    authentication
        .iter()
        .map(|(key, auth)| {
            let mut client = match auth {
                Authentication::NoAuth {} => {
                    info!("Configuring GitHub client without authentication for [{key}]");
                    Octocrab::builder().with_auth(NoAuth {})
                }
                Authentication::Token { token } => {
                    info!("Configuring GitHub client with GitHub PAT for [{key}]");
                    Octocrab::builder().personal_token(token.clone())
                }
                Authentication::App {
                    app_id,
                    private_key,
                    ..
                } => {
                    info!("Configuring GitHub client with GitHub App for [{key}]");
                    let encoding_key =
                        EncodingKey::from_rsa_pem(private_key.as_bytes()).map_err(|_| {
                            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                        "Failed to create encoding key from private key for GitHub API for [{key}]"
                    )))
                        })?;
                    Octocrab::builder().app((*app_id).into(), encoding_key)
                }
            }
            .add_header(
                USER_AGENT,
                format!("Rust/Plaid{}", env!("CARGO_PKG_VERSION")),
            )
            .build()
            .map_err(|e| ApiError::GitHubError(GitHubError::ClientError(e)))?;

            if let Authentication::App {
                installation_id, ..
            } = auth
            {
                match client.installation((*installation_id).into()) {
                    Ok(installation_client) => client = installation_client,
                    Err(e) => return Err(ApiError::GitHubError(GitHubError::ClientError(e))),
                }
            }

            Ok((key.clone(), client))
        })
        .collect()
}
