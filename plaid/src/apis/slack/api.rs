use std::collections::HashMap;

use crate::apis::{ApiError, slack::SlackError};

use super::Slack;

enum Apis {
    PostMessage,
    ViewsOpen,
}

impl std::fmt::Display for Apis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            Self::PostMessage => write!(f, "chat.postMessage"),
            Self::ViewsOpen => write!(f, "views.open"),
        }
    }
}

impl Slack {
    async fn call_slack(&self, params: &str, api: Apis) -> Result<u32, ApiError> {
        let request: HashMap<String, String> = serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // GitHub says this is only valid on Organization repositories. Not sure if it's ignored
        // on others? This may not work on standard accounts. Also, pull is the lowest permission level
        let bot = request.get("bot").ok_or(ApiError::MissingParameter("bot".to_string()))?.to_string();
        let body = request.get("body").ok_or(ApiError::MissingParameter("body".to_string()))?.to_string();

        let token = match self.config.bot_tokens.get(&bot) {
            Some(h) => format!("Bearer {h}"),
            None => {
                error!("A module tried to call api {api} for a bot that didn't exist: {bot}");
                return Err(ApiError::SlackError(SlackError::UnknownBot(bot.to_string())));
            }
        };

        info!("Calling {api} for bot: {bot}");
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
                    return Ok(0);
                }
                let response = r.text().await;
                error!("Slack data returned: {}", response.unwrap_or_default());

                return Err(ApiError::SlackError(SlackError::UnexpectedStatusCode(status.as_u16())))
            }
            Err(e) => return Err(ApiError::NetworkError(e))
        }
    }

    /// Open an arbitrary view for a configured bot. The view contents is defined by the caller but the bot
    /// must be configured in Plaid.
    pub async fn views_open(&self, params: &str,  _: &str) -> Result<u32, ApiError> {
        self.call_slack(params, Apis::ViewsOpen).await
    }

    /// Call the Slack postMessage API. The message and location are defined by the module but the bot
    /// must be configured in Plaid.
    pub async fn post_message(&self, params: &str,  _: &str) -> Result<u32, ApiError>  {
        self.call_slack(params, Apis::PostMessage).await
    }
}
