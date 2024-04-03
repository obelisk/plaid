use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::apis::{github::GitHubError, ApiError};

use super::Github;

#[derive(Serialize)]
struct GraphQLQuery {
    query: String,
    variables: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Request {
    query_name: String,
    variables: HashMap<String, String>,
}

#[derive(Serialize)]
struct AdvancedGraphQLQuery {
    query: String,
    variables: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct AdvancedRequest {
    query_name: String,
    variables: HashMap<String, serde_json::Value>,
}

const GITHUB_GQL_API: &str = "/graphql";

impl Github {
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
            query: query,
            variables: request.variables,
        };

        self.make_gql_request(query, module).await
    }

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
