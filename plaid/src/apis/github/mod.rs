mod graphql;
mod repos;
mod teams;
mod validators;

use http::{header::USER_AGENT, HeaderMap};
use jsonwebtoken::EncodingKey;
use octocrab::Octocrab;

use serde::Deserialize;

use std::collections::HashMap;

use super::ApiError;

#[derive(Deserialize)]
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
    App { app_id: u64, private_key: String },
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

pub struct Github {
    config: GithubConfig,
    client: Octocrab,
    validators: HashMap<&'static str, regex::Regex>,
}

#[derive(Debug)]
pub enum GitHubError {
    GraphQLUnserializable,
    GraphQLQueryUnknown(String),
    GraphQLInvalidCharacters(String),
    UnexpectedStatusCode(u16),
    GraphQLRequestError(String),
    ClientError(octocrab::Error),
    InvalidInput(String),
}

impl Github {
    pub fn new(config: GithubConfig) -> Self {
        let client = match &config.authentication {
            Authentication::Token { token } => Octocrab::builder().personal_token(token.clone()),
            Authentication::App {
                app_id,
                private_key,
            } => {
                let encoding_key = EncodingKey::from_rsa_pem(private_key.as_bytes())
                    .expect("Failed to create encoding key from private key for GitHub API");

                Octocrab::builder().app((*app_id).into(), encoding_key)
            }
        }
        .build()
        .unwrap();

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
        module: &str,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        info!("Making a get request to {uri} on behalf of {module}");

        // Create a header map to track the Plaid version in logs that are
        // created by this request.
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::ACCEPT,
            "application/vnd.github.v3+json".parse().unwrap(),
        );
        let version = env!("CARGO_PKG_VERSION");
        headers.insert(USER_AGENT, format!("Rust/Plaid{version}").parse().unwrap());

        let request = self.client._get_with_headers(uri, Some(headers)).await;

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
    async fn make_generic_put_request(
        &self,
        uri: String,
        body: Option<&str>,
        module: &str,
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
    async fn make_generic_delete_request(
        &self,
        uri: String,
        body: Option<&str>,
        module: &str,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        info!("Making a put request to {uri} on behalf of {module}");

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
