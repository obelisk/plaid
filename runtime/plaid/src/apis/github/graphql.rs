use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::apis::{github::GitHubError, ApiError};

use super::Github;

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

impl Github {
    /// Execute a GraphQL query by calling the GitHub API.
    async fn make_gql_request<T: Serialize>(
        &self,
        query: T,
        module: &str,
    ) -> Result<String, ApiError> {
        let request = self.client._post(GITHUB_GQL_API, Some(&query)).await;

        match request {
            Ok(r) => {
                if r.status() == 200 {
                    let body = self.client.body_to_string(r).await.map_err(|e| {
                        ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string()))
                    })?;

                    Ok(body)
                } else {
                    let status = r.status();
                    let body = self.client.body_to_string(r).await.map_err(|e| {
                        ApiError::GitHubError(GitHubError::GraphQLRequestError(e.to_string()))
                    })?;

                    warn!("Failed GraphQL query from module: {module}. Status Code: {status} Return was: {body}");
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status.as_u16(),
                    )))
                }
            }
            Err(e) => Err(ApiError::GitHubError(GitHubError::ClientError(e))),
        }
    }

    /// Execute a GraphQL query specified by `request`, on behalf of `module`.
    pub async fn make_graphql_query(
        &self,
        request: &str,
        module: &str,
    ) -> Result<String, ApiError> {
        let request: Request = serde_json::from_str(request).map_err(|_| ApiError::BadRequest)?;

        let query = match self.config.graphql_queries.get(&request.query_name) {
            Some(query) => query.to_owned(),
            None => {
                return Err(ApiError::GitHubError(GitHubError::GraphQLQueryUnknown(
                    request.query_name,
                )))
            }
        };

        let query = GraphQLQuery {
            query,
            variables: request.variables,
        };

        self.make_gql_request(query, module).await
    }

    /// Execute an advanced GraphQL query specified by `request`, on behalf of `module`.
    pub async fn make_advanced_graphql_query(
        &self,
        request: &str,
        module: &str,
    ) -> Result<String, ApiError> {
        let request: AdvancedRequest =
            serde_json::from_str(request).map_err(|_| ApiError::BadRequest)?;

        let query = match self.config.graphql_queries.get(&request.query_name) {
            Some(query) => query.to_owned(),
            None => {
                return Err(ApiError::GitHubError(GitHubError::GraphQLQueryUnknown(
                    request.query_name,
                )))
            }
        };

        let query = AdvancedGraphQLQuery {
            query: query,
            variables: request.variables,
        };

        self.make_gql_request(query, module).await
    }
}
