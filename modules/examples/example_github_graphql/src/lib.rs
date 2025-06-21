use plaid_stl::{entrypoint_with_source_and_response, github, messages::LogSource, plaid};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::{collections::HashMap, error::Error, fmt::Display};

#[derive(Debug)]
enum Errors {
    BadSender = 1,
    BadConfiguration,
    BadAuthentication,
    NoOrganization,
    NetworkFailure,
    UnknownFailure,
}

impl Display for Errors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Error for Errors {}

// This is configured in the runtime configuration within the api.toml
// (because it is part of the GitHub API configuration)
const GITHUB_GRAPHQL_QUERY: &str = "saml_identities";

entrypoint_with_source_and_response!();

#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: String,
}

#[derive(Deserialize)]
struct SamlIdentity {
    #[serde(rename = "nameId")]
    name_id: Option<String>,
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct Node {
    user: Option<User>,
    #[serde(rename = "samlIdentity")]
    saml_identity: SamlIdentity,
}

#[derive(Deserialize)]
struct ExternalIdentities {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Vec<Node>,
}

#[derive(Serialize)]
struct ReturnData {
    found_users: HashMap<String, String>,
    emails_no_users: Vec<String>,
    users_no_emails: Vec<String>,
}

fn simple_const_time_compare(a: &str, b: &str) -> bool {
    // Compare lengths first to short-circuit if they are not equal
    if a.len() != b.len() {
        return false;
    }
    // Compare each character in constant time
    a.chars()
        .zip(b.chars())
        .fold(0, |acc, (x, y)| acc | (x as u8 ^ y as u8))
        == 0
}

fn main(_: String, source: LogSource) -> Result<Option<String>, Errors> {
    // This module should only be called from a webhook POST request
    match source {
        LogSource::WebhookGet(_) => {}
        _ => return Err(Errors::BadSender),
    };

    let true_auth_token = plaid::get_secrets("organization_fetch_auth_token")
        .map_err(|_| Errors::BadConfiguration)?;

    let provided_auth_token =
        plaid::get_headers("Authorization").map_err(|_| Errors::BadAuthentication)?;

    if !simple_const_time_compare(&true_auth_token, &provided_auth_token) {
        return Err(Errors::BadAuthentication);
    }

    let organization =
        plaid::get_query_params("organization").map_err(|_| Errors::NoOrganization)?;

    // Hold all the results as a mapping of user ID to email
    let mut found_users: HashMap<String, String> = HashMap::new();
    let mut emails_no_users = Vec::new();
    let mut users_no_emails = Vec::new();
    let mut previous_cursor = String::new();
    // We need to make multiple requests because there could be more than 100 users in
    // an organization, and the GraphQL API returns a maximum of 100 results.
    loop {
        let variables = [
            ("organization".to_string(), organization.clone()),
            ("cursor".to_string(), previous_cursor.clone()),
        ]
        .into();

        match github::make_graphql_query(GITHUB_GRAPHQL_QUERY, variables) {
            Ok(result) => {
                let result: Value = serde_json::from_str(&result).unwrap();

                let ext_ident = result
                    .get("data")
                    .and_then(|v| v.get("organization"))
                    .and_then(|v| v.get("samlIdentityProvider"))
                    .and_then(|v| v.get("externalIdentities"));

                let ext_ident: ExternalIdentities =
                    serde_json::from_value(ext_ident.unwrap().clone()).map_err(|e| {
                        plaid::print_debug_string(&format!(
                            "Failed to deserialize external identities: {e}",
                        ));
                        Errors::NetworkFailure
                    })?;

                for user in ext_ident.nodes {
                    match (user.user, user.saml_identity.name_id) {
                        (Some(u), Some(email)) => {
                            // We have both a user and an email
                            found_users.insert(u.login, email);
                        }
                        (Some(u), None) => {
                            // We have a user but no email
                            users_no_emails.push(u.login);
                        }
                        (None, Some(email)) => {
                            // We have an email but no user
                            emails_no_users.push(email);
                        }
                        (None, None) => {
                            // Neither user nor email
                            continue;
                        }
                    }
                }
                if !ext_ident.page_info.has_next_page {
                    return Ok(Some(
                        serde_json::to_string(&ReturnData {
                            found_users,
                            emails_no_users,
                            users_no_emails,
                        })
                        .unwrap(),
                    ));
                }
                previous_cursor = ext_ident.page_info.end_cursor;
            }
            _ => return Err(Errors::UnknownFailure),
        }
    }
}
