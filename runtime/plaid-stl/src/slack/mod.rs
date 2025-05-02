use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

#[derive(Serialize)]
pub struct SlackMessage {
    channel: String,
    text: String,
}

#[derive(Serialize)]
pub struct SlackMessageWithBlocks {
    channel: String,
    blocks: String,
}

#[inline]
fn slack_format(msg: &str) -> String {
    #[derive(Serialize)]
    struct SlackText {
        text: String,
    }

    let new_message = SlackText {
        text: msg.to_string(),
    };

    serde_json::to_string(&new_message).unwrap()
}

pub fn post_text_to_arbitrary_webhook(webhook: &str, log: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(slack, post_to_arbitrary_webhook);
    }

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("hook", webhook.to_string());
    params.insert("body", slack_format(log));
    let params = serde_json::to_string(&params).unwrap();

    let res = unsafe { slack_post_to_arbitrary_webhook(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

pub fn post_text_to_webhook(name: &str, log: &str) -> Result<(), i32> {
    extern "C" {
        new_host_function!(slack, post_to_named_webhook);
    }

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("hook_name", name.to_string());
    params.insert("body", slack_format(log));
    let params = serde_json::to_string(&params).unwrap();

    let res = unsafe { slack_post_to_named_webhook(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

pub fn post_raw_text_to_webhook(name: &str, log: &str) -> Result<(), i32> {
    extern "C" {
        new_host_function!(slack, post_to_named_webhook);
    }

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("hook_name", name.to_string());
    params.insert("body", log.to_string());
    let params = serde_json::to_string(&params).unwrap();

    let res = unsafe { slack_post_to_named_webhook(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Data to be sent to the Plaid runtime for posting a message
#[derive(Serialize, Deserialize)]
pub struct PostMessage {
    pub bot: String,
    pub body: String,
}

pub fn post_message(bot: &str, channel: &str, text: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(slack, post_message);
    }

    let message = SlackMessage {
        channel: channel.to_owned(),
        text: text.to_owned(),
    };

    let params = serde_json::to_string(&PostMessage {
        bot: bot.to_string(),
        body: serde_json::to_string(&message).unwrap(),
    })
    .unwrap();

    let res = unsafe { slack_post_message(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

pub fn post_message_with_blocks(
    bot: &str,
    channel: &str,
    text: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(slack, post_message);
    }

    let message = SlackMessageWithBlocks {
        channel: channel.to_owned(),
        blocks: text.to_owned(),
    };

    let params = serde_json::to_string(&PostMessage {
        bot: bot.to_string(),
        body: serde_json::to_string(&message).unwrap(),
    })
    .unwrap();

    let res = unsafe { slack_post_message(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Data to be sent to the runtime to open a view
#[derive(Serialize, Deserialize)]
pub struct ViewOpen {
    pub bot: String,
    pub body: String,
}

pub fn views_open(bot: &str, view: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(slack, views_open);
    }

    let params = serde_json::to_string(&ViewOpen {
        bot: bot.to_string(),
        body: view.to_string(),
    })
    .unwrap();

    let res = unsafe { slack_views_open(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Data to be sent to the runtime for getting a user's ID from their email address
#[derive(Serialize, Deserialize)]
pub struct GetIdFromEmail {
    pub bot: String,
    pub email: String,
}

/// Get a user's ID from their email address
pub fn get_id_from_email(bot: &str, email: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, get_id_from_email);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&GetIdFromEmail {
        bot: bot.to_string(),
        email: email.to_string(),
    })
    .unwrap();

    let res = unsafe {
        slack_get_id_from_email(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    let res = String::from_utf8(return_buffer).unwrap();

    Ok(res)
}

/// Data to be sent to the runtime for getting a user's presence status
#[derive(Serialize, Deserialize)]
pub struct GetPresence {
    pub bot: String,
    pub id: String,
}

#[derive(Serialize, Deserialize)]
pub struct GetPresenceResponse {
    pub ok: bool,
    pub presence: String,
}

/// Get a user's presence status from their ID
pub fn get_presence(bot: &str, id: &str) -> Result<GetPresenceResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, get_presence);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&GetPresence {
        bot: bot.to_string(),
        id: id.to_string(),
    })
    .unwrap();

    let res = unsafe {
        slack_get_presence(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    let res = String::from_utf8(return_buffer).unwrap();

    // This should only happen if the Slack API returns a different structure
    // than expected. Which would be odd because to get here the runtime
    // successfully parsed the response.
    serde_json::from_str(&res).map_err(|_| PlaidFunctionError::Unknown)
}

#[derive(Serialize, Deserialize)]
pub struct SlackUser {
    pub id: String,
    pub team_id: String,
    pub name: String,
    pub deleted: bool,
    pub color: String,
    pub real_name: String,
    pub tz: String,
    pub tz_label: String,
    pub tz_offset: i32,
    pub profile: SlackUserProfile,
    pub is_admin: bool,
    pub is_owner: bool,
    pub is_primary_owner: bool,
    pub is_restricted: bool,
    pub is_ultra_restricted: bool,
    pub is_bot: bool,
    pub is_app_user: bool,
    pub updated: i32,
}

#[derive(Serialize, Deserialize)]
pub struct SlackUserProfile {
    pub status_text: String,
    pub status_emoji: String,
}

/// Data to be sent to the runtime for getting a user's presence status
#[derive(Serialize, Deserialize)]
pub struct UserInfo {
    pub bot: String,
    pub id: String,
}

#[derive(Serialize, Deserialize)]
pub struct UserInfoResponse {
    pub ok: bool,
    pub user: SlackUser,
}

/// Get a user's info from their ID
pub fn user_info(bot: &str, id: &str) -> Result<UserInfoResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, user_info);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&GetPresence {
        bot: bot.to_string(),
        id: id.to_string(),
    })
    .unwrap();

    let res = unsafe {
        slack_user_info(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    let res = String::from_utf8(return_buffer).unwrap();

    // This should only happen if the Slack API returns a different structure
    // than expected. Which would be odd because to get here the runtime
    // successfully parsed the response.
    serde_json::from_str(&res).map_err(|_| PlaidFunctionError::Unknown)
}
