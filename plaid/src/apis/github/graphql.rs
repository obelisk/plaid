use std::collections::HashMap;

use serde::{Serialize, Deserialize};

use crate::{apis::{ApiError, github::GitHubError}};

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

const GITHUB_GQL_API: &str = "https://api.github.com/graphql";


impl Github {
    pub async fn make_graphql_query(&self, request: &str, _: &str) -> Result<String, ApiError> {
        let request: Request = serde_json::from_str(request).map_err(|_| ApiError::BadRequest)?;

        let query = match self.config.graphql_queries.get(&request.query_name) {
            Some(query) => query.to_owned(),
            None => return Err(ApiError::GitHubError(GitHubError::GraphQLQueryUnknown(request.query_name))),
        };

        let query = serde_json::to_string(&GraphQLQuery{query: query, variables: request.variables}).map_err(|_| ApiError::GitHubError(GitHubError::GraphQLUnserializable))?;
        
        let request = self.client
            .post(GITHUB_GQL_API)
            .header("User-Agent", "Rust/Plaid")
            .header("Accept", "application/vnd.github.v3+json")
            .header("Authorization", format!("token {}", self.config.token))
            .header("Content-Type", "application/json; charset=utf-8")
            .body(query.to_string());

        match request.send().await {
            Ok(r) => if r.status() == 200 {
                Ok(r.text().await.unwrap_or_default())
            } else {
                let status = r.status();
                println!("{}", r.text().await.unwrap());
                Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(status.as_u16())))
            },
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }

    pub async fn make_advanced_graphql_query(&self, request: &str, _: &str) -> Result<String, ApiError> {
        let request: AdvancedRequest = serde_json::from_str(request).map_err(|_| ApiError::BadRequest)?;

        let query = match self.config.graphql_queries.get(&request.query_name) {
            Some(query) => query.to_owned(),
            None => return Err(ApiError::GitHubError(GitHubError::GraphQLQueryUnknown(request.query_name))),
        };

        let query = serde_json::to_string(&AdvancedGraphQLQuery{query: query, variables: request.variables}).map_err(|_| ApiError::GitHubError(GitHubError::GraphQLUnserializable))?;
        
        let request = self.client
            .post(GITHUB_GQL_API)
            .header("User-Agent", "Rust/Plaid")
            .header("Accept", "application/vnd.github.v3+json")
            .header("Authorization", format!("token {}", self.config.token))
            .header("Content-Type", "application/json; charset=utf-8")
            .body(query.to_string());

        match request.send().await {
            Ok(r) => if r.status() == 200 {
                Ok(r.text().await.unwrap_or_default())
            } else {
                let status = r.status();
                println!("{}", r.text().await.unwrap());
                Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(status.as_u16())))
            },
            Err(e) => Err(ApiError::NetworkError(e)),
        }
    }
}