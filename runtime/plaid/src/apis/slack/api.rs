use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use plaid_stl::slack::{
    ConversationsHistory, CreateChannel, CreateChannelResponse, DeleteScheduledMessage,
    GetDndInfo, GetDndInfoResponse, GetIdFromEmail, GetPresence, GetPresenceResponse,
    InviteToChannel, PostMessage, RemoveFromChannel, UpdateMessage, UserInfo, UserInfoResponse,
    ViewOpen,
};
use rand::Rng;
use reqwest::{Client, RequestBuilder};

use crate::{
    apis::{slack::SlackError, ApiError},
    loader::PlaidModule,
};

use super::Slack;

enum Apis {
    PostMessage(plaid_stl::slack::PostMessage),
    /// Internal fallback for rate limited PostMessage calls. Carries the fully
    /// rendered chat.scheduleMessage body (the original postMessage body plus `post_at`).
    ScheduleMessage(String),
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

/// Minimum lead time for a scheduled message. Keeps `post_at` safely in the
/// future (Slack rejects past timestamps with `time_in_past`) even with some
/// clock skew.
const SCHEDULE_BASE_DELAY_SECS: u64 = 30;
/// Slack rejects scheduling more than 30 messages to post within a 5 minute
/// window to the same channel (`restricted_too_many`). When that happens we
/// escalate the target window by this much and try again.
const SCHEDULE_WINDOW_SECS: u64 = 300;
/// How many windows to try before giving up (~3 hours of backlog per channel).
/// A channel that exceeds this (36 windows * 30 messages = 1080 pending
/// messages) has a problem upstream that delaying further won't fix.
const SCHEDULE_MAX_WINDOWS: u64 = 36;
/// chat.scheduleMessage is itself rate limited (Tier 3). How many times to
/// retry a single window after an HTTP 429 before giving up.
const SCHEDULE_RATE_LIMIT_RETRIES: u32 = 5;
/// How long to wait between retries when chat.scheduleMessage returns HTTP 429.
const SCHEDULE_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(2);

/// Compute the `post_at` for an escalation window. Window `n` covers
/// `[base + n*window, base + (n+1)*window)` where `base = now + SCHEDULE_BASE_DELAY_SECS`;
/// windows tile contiguously so a drained backlog is a continuous trickle, not
/// batches. `jitter` (in `[0, SCHEDULE_WINDOW_SECS)`) spreads messages inside
/// the window so a burst drips out instead of landing all at once.
fn schedule_post_at(now: u64, window: u64, jitter: u64) -> u64 {
    now + SCHEDULE_BASE_DELAY_SECS + window * SCHEDULE_WINDOW_SECS + jitter
}

/// This struct is used to deserialize a response from Slack API and
/// just check if the result is OK or not.
#[derive(serde::Deserialize)]
struct GenericSlackResponse {
    ok: bool,
}

/// Slack API response carrying the error code when `ok` is false.
#[derive(serde::Deserialize)]
struct SlackStatusResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

impl Apis {
    fn build_request(&self, client: &Client) -> RequestBuilder {
        match self {
            Self::PostMessage(p) => client
                .post(format!("{SLACK_API_URL}{api}", api = "chat.postMessage"))
                .body(p.body.clone())
                .header("Content-Type", "application/json; charset=utf-8"),
            Self::ScheduleMessage(body) => client
                .post(format!("{SLACK_API_URL}{api}", api = "chat.scheduleMessage"))
                .body(body.clone())
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
    /// channel, so a burst of posts to one channel gets HTTP 429s. Rather than
    /// surfacing those as errors (historically: dropped messages), we fall back
    /// to chat.scheduleMessage and let Slack deliver the message shortly after —
    /// see [`Self::schedule_message_fallback`]. Callers can distinguish the two
    /// outcomes by the response body: an immediate post carries `ts`, a
    /// scheduled one carries `scheduled_message_id` and `post_at`.
    pub async fn post_message(&self, params: &str, module: Arc<PlaidModule>) -> Result<String> {
        let p: PostMessage = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        let bot = p.bot.clone();
        let body = p.body.clone();
        match self
            .call_slack(bot.clone(), Apis::PostMessage(p), module.clone())
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
            Ok((429, _)) => self.schedule_message_fallback(bot, &body, module).await,
            Ok((status, _)) => Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                status,
            ))),
            Err(e) => Err(e),
        }
    }

    /// Fallback for rate limited chat.postMessage calls: hand the message to
    /// Slack for scheduled delivery instead of failing.
    ///
    /// Finds the earliest 5 minute window with capacity by first-fit probing:
    /// try `now + SCHEDULE_BASE_DELAY_SECS` plus a random offset inside the
    /// window; if Slack answers `restricted_too_many` (that window already has
    /// 30 messages scheduled for this channel), step forward one window and try
    /// again. Slack is the arbiter of capacity, so this needs no local state
    /// and is safe across multiple runtime instances. The random offset spreads
    /// a burst across its window so delivery is a steady drip rather than 30
    /// messages landing on the same second.
    async fn schedule_message_fallback(
        &self,
        bot: String,
        post_body: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String> {
        // chat.scheduleMessage takes the same payload as chat.postMessage plus
        // `post_at`, so reuse the module's rendered body with post_at injected.
        let mut payload: serde_json::Value =
            serde_json::from_str(post_body).map_err(|_| ApiError::BadRequest)?;
        if !payload.is_object() {
            return Err(ApiError::BadRequest);
        }
        let channel = payload
            .get("channel")
            .and_then(|c| c.as_str())
            .unwrap_or("<unknown>")
            .to_string();

        for window in 0..SCHEDULE_MAX_WINDOWS {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock before UNIX epoch")
                .as_secs();
            let jitter = rand::rng().random_range(0..SCHEDULE_WINDOW_SECS);
            let post_at = schedule_post_at(now, window, jitter);
            payload["post_at"] = serde_json::Value::from(post_at);
            let schedule_body = payload.to_string();

            let mut rate_limit_retries = 0;
            loop {
                match self
                    .call_slack(
                        bot.clone(),
                        Apis::ScheduleMessage(schedule_body.clone()),
                        module.clone(),
                    )
                    .await?
                {
                    (200, response) => {
                        let slack_response: SlackStatusResponse = serde_json::from_str(&response)
                            .map_err(|_| {
                                ApiError::SlackError(SlackError::UnexpectedPayload(
                                    response.clone(),
                                ))
                            })?;
                        if slack_response.ok {
                            info!("PostMessage to [{channel}] was rate limited; scheduled for delivery at [{post_at}] instead (window {window})");
                            return Ok(response);
                        }
                        match slack_response.error.as_deref() {
                            // This window already has 30 messages scheduled for
                            // this channel (or our post_at drifted into the
                            // past); step forward to the next window.
                            Some("restricted_too_many") | Some("time_in_past") => break,
                            _ => {
                                return Err(ApiError::SlackError(SlackError::UnexpectedPayload(
                                    response,
                                )))
                            }
                        }
                    }
                    // chat.scheduleMessage itself is rate limited (Tier 3);
                    // back off briefly and retry the same window.
                    (429, _) => {
                        rate_limit_retries += 1;
                        if rate_limit_retries > SCHEDULE_RATE_LIMIT_RETRIES {
                            return Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                                429,
                            )));
                        }
                        tokio::time::sleep(SCHEDULE_RATE_LIMIT_BACKOFF).await;
                    }
                    (status, _) => {
                        return Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                            status,
                        )))
                    }
                }
            }
        }

        warn!("PostMessage to [{channel}] was rate limited and no scheduling capacity was found within {SCHEDULE_MAX_WINDOWS} windows");
        Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(429)))
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
                serde_json::from_str::<SlackStatusResponse>(&response).map_err(|_| {
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

#[cfg(test)]
mod schedule_tests {
    use super::*;

    const NOW: u64 = 1_750_000_000;

    #[test]
    fn first_window_starts_at_base_delay() {
        // The earliest possible scheduled delivery is now + base delay.
        assert_eq!(schedule_post_at(NOW, 0, 0), NOW + SCHEDULE_BASE_DELAY_SECS);
    }

    #[test]
    fn windows_tile_contiguously() {
        // The latest post_at in window n is immediately before the earliest
        // post_at in window n+1, with no gap: a draining backlog is a
        // continuous trickle, not spaced batches.
        for window in 0..SCHEDULE_MAX_WINDOWS - 1 {
            let latest_in_window = schedule_post_at(NOW, window, SCHEDULE_WINDOW_SECS - 1);
            let earliest_in_next = schedule_post_at(NOW, window + 1, 0);
            assert_eq!(latest_in_window + 1, earliest_in_next);
        }
    }

    #[test]
    fn escalation_is_monotonic_across_windows() {
        // Escalating always moves post_at forward regardless of jitter,
        // preserving FIFO at window granularity.
        let worst_case_early = schedule_post_at(NOW, 1, 0);
        let best_case_late = schedule_post_at(NOW, 0, SCHEDULE_WINDOW_SECS - 1);
        assert!(worst_case_early > best_case_late);
    }

    #[test]
    fn ladder_covers_expected_backlog() {
        // 36 windows of 5 minutes ≈ 3 hours of per-channel backlog.
        let last = schedule_post_at(NOW, SCHEDULE_MAX_WINDOWS - 1, SCHEDULE_WINDOW_SECS - 1);
        let horizon = last - NOW;
        assert!(horizon >= 3 * 60 * 60);
        assert!(horizon < 4 * 60 * 60);
    }

    #[test]
    fn slack_status_response_parses_error_codes() {
        let full: SlackStatusResponse =
            serde_json::from_str(r#"{"ok":false,"error":"restricted_too_many"}"#).unwrap();
        assert!(!full.ok);
        assert_eq!(full.error.as_deref(), Some("restricted_too_many"));

        let ok: SlackStatusResponse = serde_json::from_str(
            r#"{"ok":true,"scheduled_message_id":"Q1298393284","post_at":1750000030}"#,
        )
        .unwrap();
        assert!(ok.ok);
        assert!(ok.error.is_none());
    }

    #[test]
    fn post_at_injection_preserves_payload() {
        // The fallback reuses the postMessage body with post_at added; make
        // sure the surgery keeps the original fields intact.
        let mut payload: serde_json::Value =
            serde_json::from_str(r#"{"channel":"C012345","blocks":"[]","thread_ts":"123.456"}"#)
                .unwrap();
        payload["post_at"] = serde_json::Value::from(schedule_post_at(NOW, 0, 10));
        let out = payload.to_string();
        let round_trip: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(round_trip["channel"], "C012345");
        assert_eq!(round_trip["thread_ts"], "123.456");
        assert_eq!(
            round_trip["post_at"],
            serde_json::Value::from(NOW + SCHEDULE_BASE_DELAY_SECS + 10)
        );
    }
}
