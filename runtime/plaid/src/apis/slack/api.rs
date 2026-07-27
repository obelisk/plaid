use std::sync::Arc;

use plaid_stl::slack::{
    ConversationsHistory, CreateChannel, CreateChannelResponse, DeleteScheduledMessage,
    GetDndInfo, GetDndInfoResponse, GetIdFromEmail, GetPresence, GetPresenceResponse,
    InviteToChannel, PostMessage, RemoveFromChannel, ScheduleMessage, UpdateMessage, UserInfo,
    UserInfoResponse, ViewOpen,
};
use reqwest::{Client, RequestBuilder};

use crate::{
    apis::{slack::SlackError, ApiError},
    loader::PlaidModule,
};

use super::Slack;

enum Apis {
    PostMessage(plaid_stl::slack::PostMessage),
    ScheduleMessage(plaid_stl::slack::ScheduleMessage),
    DeleteScheduledMessage(plaid_stl::slack::DeleteScheduledMessage),
    ConversationsHistory(plaid_stl::slack::ConversationsHistory),
    UpdateMessage(plaid_stl::slack::UpdateMessage),
    ViewsOpen(plaid_stl::slack::ViewOpen),
    LookupByEmail(plaid_stl::slack::GetIdFromEmail),
    GetPresence(plaid_stl::slack::GetPresence),
    GetDndInfo(plaid_stl::slack::GetDndInfo),
    UserInfo(plaid_stl::slack::UserInfo),
    CreateChannel(plaid_stl::slack::CreateChannel),
    InviteToChannel(plaid_stl::slack::InviteToChannel),
    RemoveFromChannel(plaid_stl::slack::RemoveFromChannel),
}

const SLACK_API_URL: &str = "https://slack.com/api/";
type Result<T> = std::result::Result<T, ApiError>;

/// This struct is used to deserialize a response from Slack API and
/// just check if the result is OK or not.
#[derive(serde::Deserialize)]
struct GenericSlackResponse {
    ok: bool,
}

/// Request shape for [`Slack::post_message`]: the standard `PostMessage` fields
/// plus a per-call opt-in. Parsed in a single pass since the flag rides in the
/// same JSON as the message.
#[derive(serde::Deserialize)]
struct PostMessageParams {
    bot: String,
    body: String,
    /// When true, a 429 returns Slack's rate-limited body to the caller instead
    /// of erroring, so the caller can react (e.g. schedule the message).
    /// Defaults off, preserving the historical hard-error behavior.
    #[serde(default)]
    surface_rate_limit: bool,
}

impl Apis {
    fn build_request(&self, client: &Client) -> RequestBuilder {
        match self {
            Self::PostMessage(p) => client
                .post(format!("{SLACK_API_URL}{api}", api = "chat.postMessage"))
                .body(p.body.clone())
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::ScheduleMessage(p) => client
                .post(format!("{SLACK_API_URL}{api}", api = "chat.scheduleMessage"))
                .body(p.body.clone())
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::DeleteScheduledMessage(p) => client
                .post(format!(
                    "{SLACK_API_URL}{api}",
                    api = "chat.deleteScheduledMessage"
                ))
                .body(p.body().unwrap_or_default()) // TODO this is not great: maybe this method should be fallible
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::ConversationsHistory(p) => {
                let mut query: Vec<(&str, String)> = vec![
                    ("channel", p.channel.clone()),
                    // Metadata is only returned when explicitly requested. Callers use it
                    // to correlate posted messages back to the event that generated them.
                    ("include_all_metadata", "true".to_string()),
                ];
                if let Some(limit) = p.limit {
                    query.push(("limit", limit.to_string()));
                }
                if let Some(oldest) = &p.oldest {
                    query.push(("oldest", oldest.clone()));
                }
                client
                    .get(format!(
                        "{SLACK_API_URL}{api}",
                        api = "conversations.history"
                    ))
                    .query(&query)
            }
            Self::UpdateMessage(p) => client
                .post(format!("{SLACK_API_URL}{api}", api = "chat.update"))
                .body(p.body.clone())
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::ViewsOpen(p) => client
                .post(format!("{SLACK_API_URL}{api}", api = "views.open"))
                .body(p.body.clone())
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::LookupByEmail(p) => client
                .get(format!("{SLACK_API_URL}{api}", api = "users.lookupByEmail"))
                .query(&[("email", &p.email)]),
            Self::GetPresence(p) => client
                .get(format!("{SLACK_API_URL}{api}", api = "users.getPresence"))
                .query(&[("user", &p.id)]),
            Self::GetDndInfo(p) => client
                .get(format!("{SLACK_API_URL}{api}", api = "dnd.info"))
                .query(&[("user", &p.id)]),
            Self::UserInfo(p) => client
                .get(format!("{SLACK_API_URL}{api}", api = "users.info"))
                .query(&[("user", &p.id)]),
            Self::CreateChannel(p) => client
                .post(format!(
                    "{SLACK_API_URL}{api}",
                    api = "conversations.create"
                ))
                .body(p.body().unwrap_or_default()) // TODO this is not great: maybe this method should be fallible
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::InviteToChannel(p) => client
                .post(format!(
                    "{SLACK_API_URL}{api}",
                    api = "conversations.invite"
                ))
                .body(p.body().unwrap_or_default()) // TODO this is not great: maybe this method should be fallible
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::RemoveFromChannel(p) => client
                .post(format!("{SLACK_API_URL}{api}", api = "conversations.kick"))
                .body(p.body().unwrap_or_default()) // TODO this is not great: maybe this method should be fallible
                .header("Content-Type", "application/json; charset=utf-8"),
        }
    }
}

impl std::fmt::Display for Apis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PostMessage(_) => write!(f, "PostMessage"),
            Self::ScheduleMessage(_) => write!(f, "ScheduleMessage"),
            Self::DeleteScheduledMessage(_) => write!(f, "DeleteScheduledMessage"),
            Self::ConversationsHistory(_) => write!(f, "ConversationsHistory"),
            Self::UpdateMessage(_) => write!(f, "UpdateMessage"),
            Self::ViewsOpen(_) => write!(f, "ViewsOpen"),
            Self::LookupByEmail(_) => write!(f, "LookupByEmail"),
            Self::GetPresence(_) => write!(f, "GetPresence"),
            Self::GetDndInfo(_) => write!(f, "GetDndInfo"),
            Self::UserInfo(_) => write!(f, "UserInfo"),
            Self::CreateChannel(_) => write!(f, "CreateChannel"),
            Self::InviteToChannel(_) => write!(f, "InviteToChannel"),
            Self::RemoveFromChannel(_) => write!(f, "RemoveFromChannel"),
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
            Ok((200, response)) => {
                let slack_response: GenericSlackResponse = serde_json::from_str(&response)
                    .map_err(|_| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(response.clone()))
                    })?;
                if !slack_response.ok {
                    return Err(ApiError::SlackError(SlackError::UnexpectedPayload(
                        response,
                    )));
                }
                Ok(0)
            }
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Call the Slack postMessage API. The message and location are defined by the module but the bot
    /// must be configured in Plaid.
    ///
    /// Slack rate limits chat.postMessage to roughly one message per second per
    /// channel, so a burst of posts to one channel gets HTTP 429s. A caller can
    /// opt in (via `surface_rate_limit` in the request) to receive Slack's
    /// rate-limited response body on 429 (which carries `error: "ratelimited"`)
    /// instead of a hard error, so it can decide how to react — typically by
    /// scheduling the message with [`Self::schedule_message`]. How to handle the
    /// rate limit (delay, windows, retries) is intentionally left to the rule,
    /// not baked into the runtime.
    pub async fn post_message(&self, params: &str, module: Arc<PlaidModule>) -> Result<String> {
        // Single parse: message fields plus the per-call opt-in. A malformed
        // `surface_rate_limit` now fails as BadRequest rather than being
        // silently ignored.
        let p: PostMessageParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let bot = p.bot.clone();
        let surface_rate_limit = p.surface_rate_limit;
        match self
            .call_slack(
                bot,
                Apis::PostMessage(PostMessage {
                    bot: p.bot,
                    body: p.body,
                }),
                module,
            )
            .await
        {
            Ok((200, response)) => {
                let slack_response: GenericSlackResponse = serde_json::from_str(&response)
                    .map_err(|_| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(response.clone()))
                    })?;
                if !slack_response.ok {
                    return Err(ApiError::SlackError(SlackError::UnexpectedPayload(
                        response,
                    )));
                }
                Ok(response)
            }
            // Pass Slack's own rate-limited response body through so the caller
            // sees the real payload (it carries `error: "ratelimited"`), rather
            // than a synthetic stand-in.
            Ok((429, response)) if surface_rate_limit => Ok(response),
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Call the Slack chat.scheduleMessage API. The caller supplies a fully
    /// rendered body (including `post_at`); the runtime just relays it.
    ///
    /// Returns the raw Slack response on a 200 even when `ok` is false, so the
    /// caller can react to `restricted_too_many` (the 30-per-5-minute-window
    /// limit) by choosing a different `post_at` — that policy lives in the rule.
    pub async fn schedule_message(&self, params: &str, module: Arc<PlaidModule>) -> Result<String> {
        let p: ScheduleMessage = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::ScheduleMessage(p), module)
            .await
        {
            // Pass the body through regardless of `ok`: the caller needs to see
            // `restricted_too_many` / `time_in_past` to pick another window.
            Ok((200, response)) => {
                serde_json::from_str::<GenericSlackResponse>(&response).map_err(|_| {
                    ApiError::SlackError(SlackError::UnexpectedPayload(response.clone()))
                })?;
                Ok(response)
            }
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Delete a scheduled message before it posts (chat.deleteScheduledMessage).
    ///
    /// Returns the raw Slack response even when `ok` is false: callers need the
    /// error code to distinguish "already posted" (`invalid_scheduled_message_id`)
    /// from real failures, since a scheduled message that has posted can no
    /// longer be deleted and must be handled differently.
    pub async fn delete_scheduled_message(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String> {
        let p: DeleteScheduledMessage =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::DeleteScheduledMessage(p), module)
            .await
        {
            Ok((200, response)) => {
                // Validate it parses as a Slack response but pass it through
                // regardless of `ok` — see doc comment.
                serde_json::from_str::<GenericSlackResponse>(&response).map_err(|_| {
                    ApiError::SlackError(SlackError::UnexpectedPayload(response.clone()))
                })?;
                Ok(response)
            }
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Fetch recent message history for a channel (conversations.history),
    /// including message metadata.
    pub async fn conversations_history(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String> {
        let p: ConversationsHistory =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::ConversationsHistory(p), module)
            .await
        {
            Ok((200, response)) => {
                let slack_response: GenericSlackResponse = serde_json::from_str(&response)
                    .map_err(|_| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(response.clone()))
                    })?;
                if !slack_response.ok {
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

    /// Call the Slack chat.update API. Updates an existing message identified by its ts.
    /// The bot must be configured in Plaid.
    pub async fn update_message(&self, params: &str, module: Arc<PlaidModule>) -> Result<String> {
        let p: UpdateMessage = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::UpdateMessage(p), module)
            .await
        {
            Ok((200, response)) => {
                let slack_response: GenericSlackResponse = serde_json::from_str(&response)
                    .map_err(|_| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(response.clone()))
                    })?;
                if !slack_response.ok {
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
                let u_response: UserInfoResponse =
                    serde_json::from_str(&response).map_err(|e| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(e.to_string()))
                    })?;
                if !u_response.ok {
                    return Err(ApiError::SlackError(SlackError::UnexpectedPayload(
                        response,
                    )));
                }
                Ok(u_response.user.id)
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

    /// Get a user's DND info from their ID
    pub async fn get_dnd(&self, params: &str, module: Arc<PlaidModule>) -> Result<String> {
        let p: GetDndInfo = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::GetDndInfo(p), module)
            .await
        {
            Ok((200, response)) => {
                let gdnd_response: GetDndInfoResponse =
                    serde_json::from_str(&response).map_err(|e| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(e.to_string()))
                    })?;
                if !gdnd_response.ok {
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

    /// Create a new Slack channel
    pub async fn create_channel(&self, params: &str, module: Arc<PlaidModule>) -> Result<String> {
        let p: CreateChannel = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::CreateChannel(p), module)
            .await
        {
            Ok((200, response)) => {
                let cc_response: CreateChannelResponse =
                    serde_json::from_str(&response).map_err(|e| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(e.to_string()))
                    })?;
                if !cc_response.ok {
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

    /// Invite users to a Slack channel
    pub async fn invite_to_channel(&self, params: &str, module: Arc<PlaidModule>) -> Result<u32> {
        let p: InviteToChannel = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::InviteToChannel(p), module)
            .await
        {
            Ok((200, response)) => {
                let invite_response: GenericSlackResponse = serde_json::from_str(&response)
                    .map_err(|e| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(e.to_string()))
                    })?;
                if !invite_response.ok {
                    return Err(ApiError::SlackError(SlackError::UnexpectedPayload(
                        response,
                    )));
                }
                Ok(0)
            }
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Remove a user from a Slack channel
    pub async fn remove_from_channel(&self, params: &str, module: Arc<PlaidModule>) -> Result<u32> {
        let p: RemoveFromChannel =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        match self
            .call_slack(p.bot.clone(), Apis::RemoveFromChannel(p), module)
            .await
        {
            Ok((200, response)) => {
                let remove_response: GenericSlackResponse = serde_json::from_str(&response)
                    .map_err(|e| {
                        ApiError::SlackError(SlackError::UnexpectedPayload(e.to_string()))
                    })?;
                if !remove_response.ok {
                    return Err(ApiError::SlackError(SlackError::UnexpectedPayload(
                        response,
                    )));
                }
                Ok(0)
            }
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }
}
