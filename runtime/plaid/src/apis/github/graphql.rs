use std::{collections::HashMap, fmt::Display, sync::Arc};

use plaid_stl::github::GithubApiWrapper;
use serde::{Deserialize, Serialize};

use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

use super::{body_read_error, Github, Method, Verdict};

/// A GraphQL query ready to be executed.
#[derive(Serialize)]
struct GraphQLQuery {
    /// The query to be executed.
    query: String,
    /// Variables to be interpolated in the query.
    variables: HashMap<String, String>,
}

/// A request to execute a GraphQL query.
#[derive(Deserialize)]
struct Request {
    /// The name of the query to be executed (must match some query in the config).
    query_name: String,
    /// Variables to be interpolated in the query.
    variables: HashMap<String, String>,
}

/// An advanced GraphQL query ready to be executed, where variables are generic JSON values.
#[derive(Serialize)]
struct AdvancedGraphQLQuery {
    /// The query to be executed.
    query: String,
    /// Variables to be interpolated in the query.
    variables: HashMap<String, serde_json::Value>,
}

/// A request to execute an advanced GraphQL query.
#[derive(Deserialize)]
struct AdvancedRequest {
    /// The name of the query to be executed (must match some query in the config).
    query_name: String,
    /// Variables to be interpolated in the query.
    variables: HashMap<String, serde_json::Value>,
}

const GITHUB_GQL_API: &str = "/graphql";

fn reports_rate_limit(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    value
        .get("errors")
        .and_then(|errors| errors.as_array())
        .is_some_and(|errors| {
            errors.iter().any(|error| {
                error.get("type").and_then(|kind| kind.as_str()) == Some("RATE_LIMITED")
            })
        })
}

impl Github {
    /// Execute a GraphQL query by calling the GitHub API.
    async fn make_gql_request<T: Serialize>(
        &self,
        client_id: impl Display,
        query: T,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let client_id = client_id.to_string();
        let client = self.clients.get(&client_id).ok_or_else(|| {
            ApiError::GitHubError(GitHubError::InvalidInput(format!(
                "Client ID not found: {}",
                client_id
            )))
        })?;

        let request = client._post(GITHUB_GQL_API, Some(&query)).await;
        let (status, snapshot, response) =
            self.capture_response(&client_id, Method::Graphql, request)?;
        let body = match client.body_to_string(response).await {
            Ok(body) => body,
            Err(e) => {
                self.record(
                    &client_id,
                    Method::Graphql,
                    status,
                    &snapshot,
                    Verdict::Failed,
                );
                return Err(body_read_error(e));
            }
        };

        let verdict = if status == 200 && reports_rate_limit(&body) {
            Verdict::GraphqlRateLimited
        } else {
            Verdict::Read
        };
        self.record(&client_id, Method::Graphql, status, &snapshot, verdict);

        if status == 200 {
            Ok(body)
        } else {
            warn!("Failed GraphQL query from module: {module}. Status Code: {status} Return was: {body}");
            Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                status.as_u16(),
            )))
        }
    }

    /// Execute a GraphQL query specified by `request`, on behalf of `module`.
    pub async fn make_graphql_query(
        &self,
        request: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<Request> =
            serde_json::from_str(request).map_err(|_| ApiError::BadRequest)?;

        let query = match self.config.graphql_queries.get(&request.params.query_name) {
            Some(query) => query.to_owned(),
            None => {
                return Err(ApiError::GitHubError(GitHubError::GraphQLQueryUnknown(
                    request.params.query_name,
                )))
            }
        };

        let query = GraphQLQuery {
            query,
            variables: request.params.variables,
        };

        self.make_gql_request(&request.client_id, query, module)
            .await
    }

    /// Execute an advanced GraphQL query specified by `request`, on behalf of `module`.
    pub async fn make_advanced_graphql_query(
        &self,
        request: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: GithubApiWrapper<AdvancedRequest> =
            serde_json::from_str(request).map_err(|_| ApiError::BadRequest)?;

        let query = match self.config.graphql_queries.get(&request.params.query_name) {
            Some(query) => query.to_owned(),
            None => {
                return Err(ApiError::GitHubError(GitHubError::GraphQLQueryUnknown(
                    request.params.query_name,
                )))
            }
        };

        let query = AdvancedGraphQLQuery {
            query: query,
            variables: request.params.variables,
        };

        self.make_gql_request(&request.client_id, query, module)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::reports_rate_limit;

    #[test]
    fn detects_rate_limit_errors() {
        assert!(reports_rate_limit(
            r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}"#
        ));
    }

    #[test]
    fn ignores_data_and_other_errors() {
        assert!(!reports_rate_limit(
            r#"{"data":{"viewer":{"login":"octocat"}}}"#
        ));
        assert!(!reports_rate_limit(r#"{"errors":[{"type":"NOT_FOUND"}]}"#));
        assert!(!reports_rate_limit("not json"));
    }
}
