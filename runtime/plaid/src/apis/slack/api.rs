use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::apis::{slack::SlackError, ApiError};

use super::Slack;

enum Apis {
    PostMessage,
    ViewsOpen,
    LookupByEmail,
}

impl std::fmt::Display for Apis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            Self::PostMessage => write!(f, "chat.postMessage"),
            Self::ViewsOpen => write!(f, "views.open"),
            Self::LookupByEmail => write!(f, "users.lookupByEmail"),
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
    fn get_token(&self, bot: &str) -> Result<String, ()> {
        let token = self.config.bot_tokens.get(bot).ok_or(())?;
        Ok(format!("Bearer {token}"))
    }

    async fn call_slack(&self, params: &str, api: Apis) -> Result<String, ApiError> {
        let request: HashMap<String, String> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let bot = request
            .get("bot")
            .ok_or(ApiError::MissingParameter("bot".to_string()))?
            .to_string();

        let token = self.get_token(&bot).map_err(|_| {
            error!("A module tried to call api {api} for a bot that didn't exist: {bot}");
            ApiError::SlackError(SlackError::UnknownBot(bot.to_string()))
        })?;

        info!("Calling {api} for bot: {bot}");
        match api {
            Apis::PostMessage | Apis::ViewsOpen => {
                // It's a POST call
                let body = request
                    .get("body")
                    .ok_or(ApiError::MissingParameter("body".to_string()))?
                    .to_string();
                match self
                    .client
                    .post(format!("https://slack.com/api/{api}"))
                    .header("Authorization", token)
                    .header("Content-Type", "application/json; charset=utf-8")
                    .body(body)
                    .send()
                    .await
                {
                    Ok(r) => {
                        let status = r.status();
                        if status == 200 {
                            return Ok("".to_string());
                        }
                        let response = r.text().await;
                        error!("Slack data returned: {}", response.unwrap_or_default());

                        Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                            status.as_u16(),
                        )))
                    }
                    Err(e) => Err(ApiError::NetworkError(e)),
                }
            }
            Apis::LookupByEmail => {
                // It's a GET call
                let email = request
                    .get("email")
                    .ok_or(ApiError::MissingParameter("email".to_string()))?
                    .to_string();
                match self
                    .client
                    .get(format!("https://slack.com/api/{api}?email={email}"))
                    .header("Authorization", token)
                    .send()
                    .await
                {
                    Ok(r) => {
                        let status = r.status();
                        if status == 200 {
                            let response = r.json::<SlackUserProfile>().await.map_err(|_| {
                                ApiError::SlackError(SlackError::UnexpectedPayload(
                                    "could not deserialize to Slack user profile".to_string(),
                                ))
                            })?;
                            return Ok(response.user.id);
                        }
                        error!("Failed to retrieve user's Slack ID");
                        Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(
                            status.as_u16(),
                        )))
                    }
                    Err(e) => Err(ApiError::NetworkError(e)),
                }
            }
        }
    }

    /// Open an arbitrary view for a configured bot. The view contents is defined by the caller but the bot
    /// must be configured in Plaid.
    pub async fn views_open(&self, params: &str, _: &str) -> Result<u32, ApiError> {
        self.call_slack(params, Apis::ViewsOpen).await.map(|_| 0)
    }

    /// Call the Slack postMessage API. The message and location are defined by the module but the bot
    /// must be configured in Plaid.
    pub async fn post_message(&self, params: &str, _: &str) -> Result<u32, ApiError> {
        self.call_slack(params, Apis::PostMessage).await.map(|_| 0)
    }

    /// Calls the Slack API to retrieve a user's Slack ID from their email address
    pub async fn get_id_from_email(&self, params: &str, _: &str) -> Result<String, ApiError> {
        self.call_slack(params, Apis::LookupByEmail).await
    }
}
