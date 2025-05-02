use std::sync::Arc;

use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};

use crate::{
    apis::{slack::SlackError, ApiError},
    loader::PlaidModule,
};

use super::Slack;

#[derive(Debug)]
enum Apis {
    PostMessage,
    ViewsOpen,
    LookupByEmail,
}

const SLACK_API_URL: &str = "https://slack.com/api/";

impl Apis {
    fn build_request(&self, client: &Client, uri_params: Option<String>) -> RequestBuilder {
        let uri_params = uri_params.map(|p| format!("?{p}")).unwrap_or_default();

        match self {
            Self::PostMessage => client
                .post(format!(
                    "{SLACK_API_URL}{api}{uri_params}",
                    api = "chat.postMessage"
                ))
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::ViewsOpen => client
                .post(format!(
                    "{SLACK_API_URL}{api}{uri_params}",
                    api = "view.open"
                ))
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::LookupByEmail => client.get(format!(
                "{SLACK_API_URL}{api}{uri_params}",
                api = "users.lookupByEmail"
            )),
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
        let mut request = api
            .build_request(&self.client, uri_params)
            .header("Authorization", self.get_token(&bot_name)?);

        if let Some(body) = body {
            request = request.body(body);
        }

        info!("Calling [{:?}] for bot: {bot_name}", api);
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
