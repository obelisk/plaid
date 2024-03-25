mod graphql;
mod repos;
mod teams;

use reqwest::Client;

use serde::Deserialize;

use std::{time::Duration, collections::HashMap};

#[derive(Deserialize)]
pub struct GithubConfig {
    token: String,
    graphql_queries: HashMap<String, String>,
}

pub struct Github {
    config: GithubConfig,
    client: Client,
}

#[derive(Debug)]
pub enum GitHubError {
    GraphQLUnserializable,
    GraphQLQueryUnknown(String),
    GraphQLInvalidCharacters(String),
    UnexpectedStatusCode(u16)
}

impl Github {
    pub fn new(config: GithubConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build().unwrap();

        Self {
            config,
            client,
        }
    }
}
