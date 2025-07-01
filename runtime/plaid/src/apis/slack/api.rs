use std::sync::Arc;

use plaid_stl::slack::{
    GetIdFromEmail, GetPresence, GetPresenceResponse, PostMessage, UserInfo, UserInfoResponse,
    ViewOpen,
};
use reqwest::{Client, RequestBuilder};

use crate::{
    apis::{slack::SlackError, ApiError},
    loader::PlaidModule,
};

use super::Slack;

enum Apis {
    PostMessage(plaid_stl::slack::PostMessage),
    ViewsOpen(plaid_stl::slack::ViewOpen),
    LookupByEmail(plaid_stl::slack::GetIdFromEmail),
    GetPresence(plaid_stl::slack::GetPresence),
    UserInfo(plaid_stl::slack::UserInfo),
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
                .post(format!("{SLACK_API_URL}{api}", api = "views.open"))
                .body(p.body.clone())
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::LookupByEmail(p) => client.get(format!(
                "{SLACK_API_URL}{api}?email={email}",
                api = "users.lookupByEmail",
                email = p.email,
            )),
            Self::GetPresence(p) => client.get(format!(
                "{SLACK_API_URL}{api}?user={user}",
                api = "users.getPresence",
                user = p.id,
            )),
            Self::UserInfo(p) => client.get(format!(
                "{SLACK_API_URL}{api}?user={user}",
                api = "users.info",
                user = p.id,
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
            Self::GetPresence(_) => write!(f, "GetPresence"),
            Self::UserInfo(_) => write!(f, "UserInfo"),
        }
    }
}

impl Slack {
    /// Get token for a bot, if present
    fn get_token(&self, bot: &str) -> Result<&String> {
        self.config
            .bot_tokens
            .get(bot)
            .ok_or(ApiError::SlackError(SlackError::UnknownBot(
                bot.to_string(),
            )))
    }

    /// Make a call to the Slack API
    async fn call_slack(
        &self,
        bot: String,
        api: Apis,
        module: Arc<PlaidModule>,
    ) -> Result<(u16, String)> {
        let r = api
            .build_request(&self.client)
            .header("Authorization", format!("Bearer {}", self.get_token(&bot)?));

        info!("Calling [{api}] using bot: [{bot}] on behalf of: [{module}]");
        let resp = r.send().await.map_err(|e| ApiError::NetworkError(e))?;
        let status = resp.status();
        let response = resp.text().await.unwrap_or_default();
        trace!("Slack returned: {status}: {response}");
        Ok((status.as_u16(), response))
    }

    /// Open an arbitrary view for a configured bot. The view contents is defined by the caller but the bot
    /// must be configured in Plaid.
    pub async fn views_open(&self, params: &str, module: Arc<PlaidModule>) -> Result<u32> {
        let p: ViewOpen = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::ViewsOpen(p), module)
            .await
        {
            Ok((200, _)) => Ok(0),
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Call the Slack postMessage API. The message and location are defined by the module but the bot
    /// must be configured in Plaid.
    pub async fn post_message(&self, params: &str, module: Arc<PlaidModule>) -> Result<u32> {
        let p: PostMessage = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::PostMessage(p), module)
            .await
        {
            Ok((200, _)) => Ok(0),
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Calls the Slack API to retrieve a user's Slack ID from their email address
    pub async fn get_id_from_email(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String> {
        let p: GetIdFromEmail = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::LookupByEmail(p), module)
            .await
        {
            Ok((200, response)) => {
                let response: UserInfoResponse = serde_json::from_str(&response).map_err(|e| {
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

    /// Get a user's presence status from their ID
    pub async fn get_presence(&self, params: &str, module: Arc<PlaidModule>) -> Result<String> {
        let p: GetPresence = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::GetPresence(p), module)
            .await
        {
            Ok((200, response)) => {
                let gp_response: GetPresenceResponse =
                    serde_json::from_str(&response).map_err(|e| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(e.to_string()))
                    })?;
                if !gp_response.ok {
                    return Err(ApiError::SlackError(SlackError::UnexpectedPayload(
                        response,
                    )));
                }
                Ok(response)
            }
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Get a user's info from their ID
    pub async fn user_info(&self, params: &str, module: Arc<PlaidModule>) -> Result<String> {
        let p: UserInfo = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::UserInfo(p), module)
            .await
        {
            Ok((200, response)) => {
                let up_response: UserInfoResponse =
                    serde_json::from_str(&response).map_err(|e| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(e.to_string()))
                    })?;
                if !up_response.ok {
                    return Err(ApiError::SlackError(SlackError::UnexpectedPayload(
                        response,
                    )));
                }
                Ok(response)
            }
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }
}
