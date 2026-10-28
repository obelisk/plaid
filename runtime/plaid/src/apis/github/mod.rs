mod actions;
mod code;
mod copilot;
mod deploy_keys;
mod enterprise;
mod environments;
mod graphql;
mod members;
pub(crate) mod metrics;
mod pats;
mod pull_requests;
mod rate_limit;
mod refs;
mod repos;
mod secrets;
mod teams;
mod validators;

use http::{header::USER_AGENT, HeaderMap, Method, StatusCode};
use jsonwebtoken::EncodingKey;
use octocrab::{auth::create_jwt, NoAuth, Octocrab};

use serde::{Deserialize, Serialize};

use std::{collections::HashMap, fmt::Display, sync::Arc};

use crate::loader::PlaidModule;

pub use metrics::GithubMetrics;
use metrics::Verdict;
pub(crate) use rate_limit::RateLimitSnapshot;

use super::ApiError;

/// The `client` label for requests made with a caller-supplied token, which no
/// configured GitHub client owns.
const UNATTRIBUTED_CLIENT: &str = "unattributed";

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
    /// Raw GitHub App authentication material (app_id, private key) used to generate
    /// short-lived JWTs for endpoints that require app-level authentication.
    app_auth: HashMap<String, AppAuth>,
    /// Validators used to check parameters passed by modules
    validators: HashMap<&'static str, regex::Regex>,
    /// Rate-limit collectors, present only when the metrics endpoint is configured.
    metrics: Option<Arc<GithubMetrics>>,
}

/// Minimal information needed to authenticate as a GitHub App via JWT.
#[derive(Clone)]
pub struct AppAuth {
    pub app_id: u64,
    pub key: EncodingKey,
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
    pub(crate) fn with_metrics(
        config: GithubConfig,
        metrics: Option<Arc<GithubMetrics>>,
    ) -> Result<Self, ApiError> {
        let clients = build_github_clients(&config.authentication)?;
        let app_auth = build_app_auth(&config.authentication)?;

        // Create all the validators and compile all the regexes. If the module contains
        // any invalid regexes it will panic.
        let validators = validators::create_validators();

        Ok(Self {
            config,
            clients,
            app_auth,
            validators,
            metrics,
        })
    }

    /// Capture a response's rate-limit headers before the body is consumed and
    /// count send failures. Recording waits for `finish` so the caller's
    /// verdict decides whether a 2XX can count as `success`.
    fn capture_response<B>(
        &self,
        client: &str,
        method: &Method,
        request: octocrab::Result<http::Response<B>>,
    ) -> Result<(StatusCode, RateLimitSnapshot, http::Response<B>), ApiError> {
        match request {
            Ok(response) => {
                let snapshot = RateLimitSnapshot::from_headers(response.headers());
                Ok((response.status(), snapshot, response))
            }
            Err(e) => {
                self.record_transport_error(client, method);
                Err(ApiError::GitHubError(GitHubError::ClientError(e)))
            }
        }
    }

    /// Record the captured response and hand the caller its usual status/body
    /// pair.
    fn finish(
        &self,
        client: &str,
        method: &Method,
        status: StatusCode,
        snapshot: &RateLimitSnapshot,
        body: Result<String, ApiError>,
    ) -> (u16, Result<String, ApiError>) {
        let verdict = Verdict::observed(status, snapshot, body.as_deref().ok());
        self.record(client, method, status, snapshot, verdict);
        (status.as_u16(), body)
    }

    fn record(
        &self,
        client: &str,
        method: &Method,
        status: StatusCode,
        snapshot: &RateLimitSnapshot,
        verdict: Verdict,
    ) {
        if let Some(metrics) = self.metrics.as_deref() {
            metrics.record(client, method, status, snapshot, verdict);
        }
    }

    fn record_transport_error(&self, client: &str, method: &Method) {
        if let Some(metrics) = self.metrics.as_deref() {
            metrics.record_transport_error(client, method);
        }
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

        let client_id = client_id.to_string();
        let client = self.clients.get(&client_id).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                client_id
            )))
        })?;

        let request = client._get_with_headers(uri, headers).await;
        let (status, snapshot, response) =
            self.capture_response(&client_id, &Method::GET, request)?;
        let body = client
            .body_to_string(response)
            .await
            .map_err(body_read_error);
        Ok(self.finish(&client_id, &Method::GET, status, &snapshot, body))
    }

    /// Make a generic post request to the GitHub API using the GitHub app library. This exists
    /// to help facilitate the conversion from a token usage to GitHub app. It also means that
    /// extra parsing can be avoided since we need to re-serialize anyway to pass back to the rules
    /// (at least currently).
    async fn make_generic_post_request<T: Serialize>(
        &self,
        client_id: impl Display + Clone,
        uri: String,
        body: T,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        self.make_generic_post_request_with_headers(client_id, uri, body, None, module)
            .await
    }

    /// Make a generic post request to the GitHub API authenticated as the GitHub App (using a JWT).
    /// This is useful for endpoints that require app-level authentication, such as creating an
    /// installation access token.
    async fn make_app_authenticated_post_request<T: Serialize>(
        &self,
        client_id: impl Display + Clone,
        uri: String,
        body: T,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        let headers = self.app_jwt_header(client_id.to_string())?;
        self.make_generic_post_request_with_headers(client_id, uri, body, Some(headers), module)
            .await
    }

    /// Make a generic post request with custom headers to the GitHub API using the GitHub app library.
    /// Note - This function does not do any validation on the provided headers. That's because
    /// it's not exposed to the rules but only callable from within the runtime itself. Therefore
    /// we assume that all necessary validation has already been performed by the calling function.
    async fn make_generic_post_request_with_headers<T: Serialize>(
        &self,
        client_id: impl Display + Clone,
        uri: String,
        body: T,
        headers: Option<HeaderMap>,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        // We log the header names we are passing but not the values, in case they are sensitive.
        info!(
            "Making a post request to {uri} on behalf of {module}. Provided headers: {:?}",
            match headers {
                Some(ref headers) => headers
                    .keys()
                    .map(|v| v.as_str())
                    .collect::<Vec<&str>>()
                    .join(", "),
                None => "None".to_string(),
            }
        );

        let client_id = client_id.to_string();
        let client = self.clients.get(&client_id).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                client_id
            )))
        })?;

        let response = match headers {
            // No custom headers: use the client's built-in POST helper so we don't change the
            // transport path for the existing POST-based GitHub API wrappers.
            None => client._post(uri, Some(&body)).await,
            // Custom headers provided (e.g. app-level JWT auth): build the request manually so we
            // can override headers such as Authorization before sending.
            Some(headers) => {
                let mut request = client
                    .build_request(
                        http::Request::builder().method(Method::POST).uri(&uri),
                        Some(&body),
                    )
                    .map_err(|e| ApiError::GitHubError(GitHubError::ClientError(e)))?;

                for (name, value) in headers {
                    if let Some(name) = name {
                        request.headers_mut().insert(name, value);
                    }
                }

                client.send(request).await
            }
        };

        let (status, snapshot, response) =
            self.capture_response(&client_id, &Method::POST, response)?;
        let body = client
            .body_to_string(response)
            .await
            .map_err(body_read_error);
        Ok(self.finish(&client_id, &Method::POST, status, &snapshot, body))
    }

    /// Build an `Authorization: Bearer <JWT>` header for a GitHub App client identified by
    /// `client_id`. The JWT is generated from the configured app_id and private key using
    /// octocrab's `create_jwt`. Returns an error if the client is not configured as an app.
    fn app_jwt_header(&self, client_id: String) -> Result<HeaderMap, ApiError> {
        let app_auth = self.app_auth.get(&client_id).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not configured as a GitHub App: {client_id}"
            )))
        })?;

        let app_id = app_auth.app_id.into();
        let jwt = create_jwt(app_id, &app_auth.key).map_err(|_| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Failed to generate JWT for GitHub App client: {client_id}"
            )))
        })?;

        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            format!("Bearer {jwt}").parse().map_err(|_| {
                ApiError::GitHubError(GitHubError::InvalidInput(
                    "Failed to build authorization header".to_string(),
                ))
            })?,
        );
        Ok(headers)
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

        let client_id = client_id.to_string();
        let client = self.clients.get(&client_id).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                client_id
            )))
        })?;

        let request = client._put(uri, body).await;
        let (status, snapshot, response) =
            self.capture_response(&client_id, &Method::PUT, request)?;
        let body = client
            .body_to_string(response)
            .await
            .map_err(body_read_error);
        Ok(self.finish(&client_id, &Method::PUT, status, &snapshot, body))
    }

    /// Make a generic patch request to the GitHub API using the GitHub app library. This exists
    /// to help facilitate the conversion from a token usage to GitHub app. It also means that
    /// extra parsing can be avoided since we need to re-serialize anyway to pass back to the rules
    /// (at least currently).
    async fn make_generic_patch_request<T: Serialize>(
        &self,
        client_id: impl Display,
        uri: String,
        body: Option<&T>,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        info!("Making a patch request to {uri} on behalf of {module}");

        let client_id = client_id.to_string();
        let client = self.clients.get(&client_id).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                client_id
            )))
        })?;

        let request = client._patch(uri, body).await;
        let (status, snapshot, response) =
            self.capture_response(&client_id, &Method::PATCH, request)?;
        let body = client
            .body_to_string(response)
            .await
            .map_err(body_read_error);
        Ok(self.finish(&client_id, &Method::PATCH, status, &snapshot, body))
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

        let client_id = client_id.to_string();
        let client = self.clients.get(&client_id).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                client_id
            )))
        })?;

        let request = client._delete(uri, body).await;
        let (status, snapshot, response) =
            self.capture_response(&client_id, &Method::DELETE, request)?;
        let body = client
            .body_to_string(response)
            .await
            .map_err(body_read_error);
        Ok(self.finish(&client_id, &Method::DELETE, status, &snapshot, body))
    }

    /// Make a DELETE request to the GitHub API with a custom `Authorization: Bearer <token>`
    /// header using a standalone reqwest client. This is needed for endpoints that require
    /// authenticating with a token that is different from the configured client, such as
    /// revoking an installation access token.
    async fn make_delete_request_with_token<T: Serialize>(
        &self,
        uri: String,
        body: Option<&T>,
        token: String,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, Result<String, ApiError>), ApiError> {
        info!("Making a delete request to {uri} on behalf of {module} with provided token");

        let client = reqwest::Client::new();
        let mut request = client
            .delete(&uri)
            .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
            .header(
                USER_AGENT,
                format!("Rust/Plaid{}", env!("CARGO_PKG_VERSION")),
            );

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                self.record_transport_error(UNATTRIBUTED_CLIENT, &Method::DELETE);
                return Err(ApiError::NetworkError(e));
            }
        };

        let status = response.status();
        let snapshot = RateLimitSnapshot::from_headers(response.headers());
        let body = response
            .text()
            .await
            .map_err(|e| ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string())));
        Ok(self.finish(
            UNATTRIBUTED_CLIENT,
            &Method::DELETE,
            status,
            &snapshot,
            body,
        ))
    }
}

fn body_read_error(e: octocrab::Error) -> ApiError {
    ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string()))
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

/// Extracts the raw GitHub App authentication material (app_id + private key) for each
/// configured app client. This is used to generate short-lived JWTs for app-level endpoints.
fn build_app_auth(
    authentication: &HashMap<String, Authentication>,
) -> Result<HashMap<String, AppAuth>, ApiError> {
    if authentication.is_empty() {
        return Err(ApiError::GitHubError(GitHubError::InvalidInput(
            "At least one GitHub client must be configured".to_string(),
        )));
    }

    authentication
        .iter()
        .filter_map(|(key, auth)| match auth {
            Authentication::App {
                app_id,
                private_key,
                ..
            } => {
                info!("Configuring GitHub App authentication material for [{key}]");
                let encoding_key =
                    EncodingKey::from_rsa_pem(private_key.as_bytes()).map_err(|_| {
                        ApiError::GitHubError(GitHubError::InvalidInput(format!(
                        "Failed to create encoding key from private key for GitHub API for [{key}]"
                    )))
                    });
                let encoding_key = match encoding_key {
                    Ok(key) => key,
                    Err(e) => return Some(Err(e)),
                };
                Some(Ok((
                    key.clone(),
                    AppAuth {
                        app_id: *app_id,
                        key: encoding_key,
                    },
                )))
            }
            _ => None,
        })
        .collect()
}
