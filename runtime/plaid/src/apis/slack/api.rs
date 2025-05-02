use std::sync::Arc;

use plaid_stl::slack::{GetIdFromEmail, PostMessage, ViewOpen};
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};

use crate::{
    apis::{slack::SlackError, ApiError},
    loader::PlaidModule,
};

use super::Slack;

enum Apis {
    PostMessage(plaid_stl::slack::PostMessage),
    ViewsOpen(plaid_stl::slack::ViewOpen),
    LookupByEmail(plaid_stl::slack::GetIdFromEmail),
}

const SLACK_API_URL: &str = "https://slack.com/api/";
type Result<T> = std::result::Result<T, ApiError>;

impl Apis {
    fn build_request(&self, client: &Client) -> RequestBuilder {
        match self {
            Self::PostMessage(p) => client
                .post(format!("{SLACK_API_URL}{api}", api = "chat.postMessage"))
                .body(p.body.clone())
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::ViewsOpen(p) => client
                .post(format!("{SLACK_API_URL}{api}", api = "view.open"))
                .body(p.body.clone())
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::LookupByEmail(p) => client.get(format!(
                "{SLACK_API_URL}{api}?email={email}",
                api = "users.lookupByEmail",
                email = p.email,
            )),
        }
    }
}

impl std::fmt::Display for Apis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PostMessage(_) => write!(f, "PostMessage"),
            Self::ViewsOpen(_) => write!(f, "ViewsOpen"),
            Self::LookupByEmail(_) => write!(f, "LookupByEmail"),
        }
    }
}

impl Slack {
    /// Get token for a bot, if present
    fn get_token(&self, bot: &str) -> Result<String> {
        match self.config.bot_tokens.get(bot) {
            Some(token) => Ok(format!("Bearer {token}")),
            None => Err(ApiError::SlackError(SlackError::UnknownBot(
                bot.to_string(),
            ))),
        }
    }

    /// Make a call to the Slack API
    async fn call_slack(&self, bot_name: String, api: Apis) -> Result<(u16, String)> {
        let r = api
            .build_request(&self.client)
            .header("Authorization", self.get_token(&bot_name)?);

        info!("Calling [{api}] for bot: {bot_name}");
        let resp = r.send().await.map_err(|e| ApiError::NetworkError(e))?;
        let status = resp.status();
        let response = resp.text().await.unwrap_or_default();
        trace!("Slack returned: {status}: {response}");
        Ok((status.as_u16(), response))
    }

    /// Open an arbitrary view for a configured bot. The view contents is defined by the caller but the bot
    /// must be configured in Plaid.
    pub async fn views_open(&self, params: &str, _: Arc<PlaidModule>) -> Result<u32> {
        let p: ViewOpen = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self.call_slack(p.bot.clone(), Apis::ViewsOpen(p)).await {
            Ok((200, _)) => Ok(0),
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Call the Slack postMessage API. The message and location are defined by the module but the bot
    /// must be configured in Plaid.
    pub async fn post_message(&self, params: &str, _: Arc<PlaidModule>) -> Result<u32> {
        let p: PostMessage = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self.call_slack(p.bot.clone(), Apis::PostMessage(p)).await {
            Ok((200, _)) => Ok(0),
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Calls the Slack API to retrieve a user's Slack ID from their email address
    pub async fn get_id_from_email(&self, params: &str, _: Arc<PlaidModule>) -> Result<String> {
        /// Slack user profile as returned by https://api.slack.com/methods/users.lookupByEmail
        #[derive(Serialize, Deserialize)]
        struct SlackUserProfile {
            user: SlackUser,
        }

        #[derive(Serialize, Deserialize)]
        struct SlackUser {
            id: String,
        }

        let p: GetIdFromEmail = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self.call_slack(p.bot.clone(), Apis::LookupByEmail(p)).await {
            Ok((200, response)) => {
                let response: SlackUserProfile = serde_json::from_str(&response).map_err(|e| {
                    ApiError::SlackError(SlackError::UnexpectedPayload(e.to_string()))
                })?;
                Ok(response.user.id)
            }
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }
}
