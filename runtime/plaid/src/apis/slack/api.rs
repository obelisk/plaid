use std::sync::Arc;

use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};

use crate::{
    apis::{slack::SlackError, ApiError},
    loader::PlaidModule,
};

use super::Slack;

enum Apis {
    PostMessage,
    ViewsOpen,
    LookupByEmail,
}

impl Apis {
    fn get_uri(&self) -> &str {
        match self {
            Self::PostMessage => "chat.postMessage",
            Self::ViewsOpen => "views.open",
            Self::LookupByEmail => "users.lookupByEmail",
        }
    }

    fn build_post_request(client: &Client, api: &Apis, uri_params: String) -> RequestBuilder {
        client
            .post(format!(
                "https://slack.com/api/{}{uri_params}",
                api.get_uri()
            ))
            .header("Content-Type", "application/json; charset=utf-8")
    }

    fn build_get_request(client: &Client, api: &Apis, uri_params: String) -> RequestBuilder {
        client.get(format!(
            "https://slack.com/api/{}{uri_params}",
            api.get_uri()
        ))
    }

    /// Create a request builder and properly configure the API, headers, and authorization token
    fn request_builder<T: AsRef<str>>(
        client: &Client,
        api: &Apis,
        token: String,
        uri_params: Option<T>,
    ) -> RequestBuilder {
        let uri_params = match uri_params {
            Some(p) => format!("?{}", p.as_ref()),
            None => String::new(),
        };

        let builder = match api {
            Apis::PostMessage => Self::build_post_request(client, api, uri_params),
            Apis::ViewsOpen => Self::build_post_request(client, api, uri_params),
            Apis::LookupByEmail => Self::build_get_request(client, api, uri_params),
        };

        builder.header("Authorization", token)
    }
}

impl std::fmt::Display for Apis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PostMessage => write!(f, "Chat Post Message"),
            Self::ViewsOpen => write!(f, "Views Open"),
            Self::LookupByEmail => write!(f, "Users Lookup By Email"),
        }
    }
}

/// Slack user profile as returned by https://api.slack.com/methods/users.lookupByEmail
#[derive(Serialize, Deserialize)]
struct SlackUserProfile {
    user: SlackUser,
}

#[derive(Serialize, Deserialize)]
struct SlackUser {
    id: String,
}

impl Slack {
    /// Get token for a bot, if present
    fn get_token(&self, bot: &str) -> Result<String, ApiError> {
        match self.config.bot_tokens.get(bot) {
            Some(token) => Ok(format!("Bearer {token}")),
            None => Err(ApiError::SlackError(SlackError::UnknownBot(
                bot.to_string(),
            ))),
        }
    }

    /// Make a call to the Slack API
    async fn call_slack(
        &self,
        bot_name: String,
        api: Apis,
        body: Option<String>,
        uri_params: Option<String>,
    ) -> Result<(u16, String), ApiError> {
        let mut request =
            Apis::request_builder(&self.client, &api, self.get_token(&bot_name)?, uri_params);

        if let Some(body) = body {
            request = request.body(body);
        }

        info!("Calling {api} for bot: {bot_name}");
        match request.send().await {
            Ok(r) => {
                let status = r.status();
                let response = r.text().await.unwrap_or_default();
                trace!("Slack returned: {status}: {response}");
                Ok((status.as_u16(), response))
            }
            Err(e) => return Err(ApiError::NetworkError(e)),
        }
    }

    /// Open an arbitrary view for a configured bot. The view contents is defined by the caller but the bot
    /// must be configured in Plaid.
    pub async fn views_open(&self, params: &str, _: Arc<PlaidModule>) -> Result<u32, ApiError> {
        let params: plaid_stl::slack::ViewOpen =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        match self
            .call_slack(params.bot, Apis::ViewsOpen, Some(params.body), None)
            .await
        {
            Ok((200, _)) => Ok(0),
            Ok((status, _)) => {
                error!("Slack returned unexpected status code: {status}");
                Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                    status,
                )))
            }
            Err(e) => Err(e),
        }
    }

    /// Call the Slack postMessage API. The message and location are defined by the module but the bot
    /// must be configured in Plaid.
    pub async fn post_message(&self, params: &str, _: Arc<PlaidModule>) -> Result<u32, ApiError> {
        let params: plaid_stl::slack::PostMessage =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(params.bot, Apis::PostMessage, Some(params.body), None)
            .await
        {
            Ok((200, _)) => Ok(0),
            Ok((status, _)) => {
                error!("Slack returned unexpected status code: {status}");
                Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                    status,
                )))
            }
            Err(e) => Err(e),
        }
    }

    /// Calls the Slack API to retrieve a user's Slack ID from their email address
    pub async fn get_id_from_email(
        &self,
        params: &str,
        _: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let params: plaid_stl::slack::GetIdFromEmail =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        match self
            .call_slack(params.bot, Apis::LookupByEmail, None, Some(params.email))
            .await
        {
            Ok((200, response)) => {
                let response: SlackUserProfile = serde_json::from_str(&response).map_err(|_| {
                    ApiError::SlackError(SlackError::UnexpectedPayload(
                        "could not deserialize to Slack user profile".to_string(),
                    ))
                })?;
                Ok(response.user.id)
            }
            Ok((status, _)) => {
                error!("Slack returned unexpected status code: {status}");
                Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                    status,
                )))
            }
            Err(e) => Err(e),
        }
    }
}
