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
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_ts: Option<String>,
}

#[derive(Serialize)]
struct SlackMessageWithBlocksAndAttachments {
    channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocks: Option<serde_json::Value>,
    attachments: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    unfurl_links: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unfurl_media: Option<bool>,
}

fn optional_blocks(blocks: &str) -> Result<Option<serde_json::Value>, PlaidFunctionError> {
    let blocks: serde_json::Value =
        serde_json::from_str(blocks).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;

    match blocks {
        serde_json::Value::Array(items) if items.is_empty() => Ok(None),
        blocks => Ok(Some(blocks)),
    }
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

pub fn post_message_detailed(
    bot: &str,
    channel: &str,
    text: &str,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, post_message);
    }

    let message = SlackMessage {
        channel: channel.to_owned(),
        text: text.to_owned(),
    };

    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&PostMessage {
        bot: bot.to_string(),
        body: serde_json::to_string(&message).unwrap(),
    })
    .unwrap();

    let res = unsafe {
        slack_post_message(
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
    let res = String::from_utf8(return_buffer).unwrap();

    Ok(res)
}

pub fn post_message(bot: &str, channel: &str, text: &str) -> Result<(), PlaidFunctionError> {
    post_message_detailed(bot, channel, text).map(|_| ())
}

pub fn post_message_with_blocks_detailed(
    bot: &str,
    channel: &str,
    text: &str,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, post_message);
    }

    let message = SlackMessageWithBlocks {
        channel: channel.to_owned(),
        blocks: text.to_owned(),
        thread_ts: None,
    };

    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&PostMessage {
        bot: bot.to_string(),
        body: serde_json::to_string(&message).unwrap(),
    })
    .unwrap();

    let res = unsafe {
        slack_post_message(
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
    let res = String::from_utf8(return_buffer).unwrap();

    Ok(res)
}

pub fn post_message_with_blocks(
    bot: &str,
    channel: &str,
    text: &str,
) -> Result<(), PlaidFunctionError> {
    post_message_with_blocks_detailed(bot, channel, text).map(|_| ())
}

/// Post a Slack message with top-level Block Kit blocks and attachments.
pub fn post_message_with_blocks_and_attachments(
    bot: &str,
    channel: &str,
    blocks: &str,
    attachments: &str,
) -> Result<SlackMessageResponse, PlaidFunctionError> {
    post_message_with_blocks_and_attachments_unfurl(bot, channel, blocks, attachments, None, None)
}

/// Post a Slack message with top-level Block Kit blocks and attachments, while
/// explicitly controlling link/media unfurling. Passing `None` for an unfurl
/// option leaves it unset, preserving Slack's default unfurling behavior, so
/// existing callers are unaffected.
pub fn post_message_with_blocks_and_attachments_unfurl(
    bot: &str,
    channel: &str,
    blocks: &str,
    attachments: &str,
    unfurl_links: Option<bool>,
    unfurl_media: Option<bool>,
) -> Result<SlackMessageResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, post_message);
    }

    let message = SlackMessageWithBlocksAndAttachments {
        channel: channel.to_owned(),
        blocks: optional_blocks(blocks)?,
        attachments: serde_json::from_str(attachments)
            .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?,
        unfurl_links,
        unfurl_media,
    };

    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&PostMessage {
        bot: bot.to_string(),
        body: serde_json::to_string(&message).unwrap(),
    })
    .unwrap();

    let res = unsafe {
        slack_post_message(
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
    let res = String::from_utf8(return_buffer).unwrap();

    serde_json::from_str(&res).map_err(|_| PlaidFunctionError::Unknown)
}

/// Slack API response for message operations (chat.postMessage, chat.update).
#[derive(Serialize, Deserialize)]
pub struct SlackMessageResponse {
    pub ok: bool,
    pub channel: String,
    pub ts: String,
}

/// Post a Block Kit message as a thread reply under an existing message.
pub fn post_message_with_blocks_in_thread(
    bot: &str,
    channel: &str,
    blocks: &str,
    thread_ts: &str,
) -> Result<SlackMessageResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, post_message);
    }

    let message = SlackMessageWithBlocks {
        channel: channel.to_owned(),
        blocks: blocks.to_owned(),
        thread_ts: Some(thread_ts.to_owned()),
    };

    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&PostMessage {
        bot: bot.to_string(),
        body: serde_json::to_string(&message).unwrap(),
    })
    .unwrap();

    let res = unsafe {
        slack_post_message(
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
    let res = String::from_utf8(return_buffer).unwrap();

    serde_json::from_str(&res).map_err(|_| PlaidFunctionError::Unknown)
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

/// Data to be sent to the runtime for getting info about a user's DND status
#[derive(Serialize, Deserialize)]
pub struct GetDndInfo {
    pub bot: String,
    pub id: String,
}

#[derive(Serialize, Deserialize)]
pub struct GetDndInfoResponse {
    pub ok: bool,
    pub dnd_enabled: bool,
    pub next_dnd_start_ts: u64,
    pub next_dnd_end_ts: u64,
    // Note: there is a `snooze_enabled` property but the docs say
    //   "All of the snooze_* properties will only be visible if the user being queried is also the current user."
    // Therefore we leave it out because the typical use case is retrieving info about another user.
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

/// Get a user's DND status from their ID
pub fn get_dnd(bot: &str, id: &str) -> Result<GetDndInfoResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, get_dnd);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&GetDndInfo {
        bot: bot.to_string(),
        id: id.to_string(),
    })
    .unwrap();

    let res = unsafe {
        slack_get_dnd(
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

/// Data to be sent to the runtime for creating a channel
#[derive(Serialize, Deserialize)]
pub struct CreateChannel {
    /// Bot to use for creating the channel
    pub bot: String,
    /// Name of the channel to create. Note: Slack can still modify this
    pub name: String,
    /// Whether the channel should be private
    pub is_private: bool,
}

impl CreateChannel {
    /// Serialize the body for the Slack API request
    pub fn body(&self) -> Result<String, String> {
        #[derive(Serialize)]
        struct CreateChannelBody<'a> {
            name: &'a str,
            is_private: bool,
        }

        let body = CreateChannelBody {
            name: &self.name,
            is_private: self.is_private,
        };

        serde_json::to_string(&body).map_err(|e| format!("Failed to serialize body: {}", e))
    }
}

/// A Slack channel
#[derive(Serialize, Deserialize)]
pub struct SlackChannel {
    pub id: String,
    pub name: String,
}

/// Response from Slack when creating a channel
#[derive(Serialize, Deserialize)]
pub struct CreateChannelResponse {
    pub ok: bool,
    pub channel: SlackChannel,
}

/// Create a Slack channel
/// - `bot`: bot to use for creating the channel
/// - `name`: name of the channel to create
/// - `is_private`: whether the channel should be private
pub fn create_channel(
    bot: &str,
    name: &str,
    is_private: bool,
) -> Result<CreateChannelResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, create_channel);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&CreateChannel {
        bot: bot.to_string(),
        name: name.to_string(),
        is_private,
    })
    .unwrap();

    let res = unsafe {
        slack_create_channel(
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

/// Data to be sent to the runtime for inviting users to a channel
#[derive(Serialize, Deserialize)]
pub struct InviteToChannel {
    /// Bot to use for inviting users
    pub bot: String,
    /// ID of the channel to invite users to
    pub channel: String,
    /// IDs of the users to invite
    pub users: Vec<String>,
}

impl InviteToChannel {
    /// Serialize the body for the Slack API request
    pub fn body(&self) -> Result<String, String> {
        #[derive(Serialize)]
        struct InviteToChannelBody<'a> {
            channel: &'a str,
            users: String,
        }

        let users = self.users.join(",");

        let body = InviteToChannelBody {
            channel: &self.channel,
            users,
        };

        serde_json::to_string(&body).map_err(|e| format!("Failed to serialize body: {}", e))
    }
}

/// Invite users to a Slack channel
/// - `bot`: bot to use for inviting users
/// - `channel`: ID of the channel to invite users to
/// - `users`: IDs of the users to invite
pub fn invite_to_channel(
    bot: &str,
    channel: &str,
    users: Vec<&str>,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(slack, invite_to_channel);
    }

    let params = serde_json::to_string(&InviteToChannel {
        bot: bot.to_string(),
        channel: channel.to_string(),
        users: users.iter().map(|s| s.to_string()).collect(),
    })
    .unwrap();

    let res = unsafe { slack_invite_to_channel(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

#[derive(Serialize)]
struct SlackUpdateMessageWithBlocks {
    channel: String,
    ts: String,
    blocks: String,
}

#[derive(Serialize)]
struct SlackUpdateMessageWithBlocksAndAttachments {
    channel: String,
    ts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocks: Option<serde_json::Value>,
    attachments: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    unfurl_links: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unfurl_media: Option<bool>,
}

/// Data to be sent to the Plaid runtime for updating a message
#[derive(Serialize, Deserialize)]
pub struct UpdateMessage {
    pub bot: String,
    pub body: String,
}

/// Update an existing Slack message with new Block Kit content (chat.update).
/// - `bot`: configured bot name
/// - `channel`: channel ID containing the message
/// - `ts`: timestamp of the message to update
/// - `blocks`: new Block Kit JSON content
pub fn update_message_with_blocks(
    bot: &str,
    channel: &str,
    ts: &str,
    blocks: &str,
) -> Result<SlackMessageResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, update_message);
    }

    let message = SlackUpdateMessageWithBlocks {
        channel: channel.to_owned(),
        ts: ts.to_owned(),
        blocks: blocks.to_owned(),
    };

    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&UpdateMessage {
        bot: bot.to_string(),
        body: serde_json::to_string(&message).unwrap(),
    })
    .unwrap();

    let res = unsafe {
        slack_update_message(
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
    let res = String::from_utf8(return_buffer).unwrap();

    serde_json::from_str(&res).map_err(|_| PlaidFunctionError::Unknown)
}

/// Update a Slack message with top-level Block Kit blocks and attachments.
pub fn update_message_with_blocks_and_attachments(
    bot: &str,
    channel: &str,
    ts: &str,
    blocks: &str,
    attachments: &str,
) -> Result<SlackMessageResponse, PlaidFunctionError> {
    update_message_with_blocks_and_attachments_unfurl(
        bot, channel, ts, blocks, attachments, None, None,
    )
}

/// Update a Slack message with top-level Block Kit blocks and attachments, while
/// explicitly controlling link/media unfurling. Passing `None` for an unfurl
/// option leaves it unset, preserving Slack's default unfurling behavior, so
/// existing callers are unaffected.
pub fn update_message_with_blocks_and_attachments_unfurl(
    bot: &str,
    channel: &str,
    ts: &str,
    blocks: &str,
    attachments: &str,
    unfurl_links: Option<bool>,
    unfurl_media: Option<bool>,
) -> Result<SlackMessageResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, update_message);
    }

    let message = SlackUpdateMessageWithBlocksAndAttachments {
        channel: channel.to_owned(),
        ts: ts.to_owned(),
        blocks: optional_blocks(blocks)?,
        attachments: serde_json::from_str(attachments)
            .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?,
        unfurl_links,
        unfurl_media,
    };

    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&UpdateMessage {
        bot: bot.to_string(),
        body: serde_json::to_string(&message).unwrap(),
    })
    .unwrap();

    let res = unsafe {
        slack_update_message(
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
    let res = String::from_utf8(return_buffer).unwrap();

    serde_json::from_str(&res).map_err(|_| PlaidFunctionError::Unknown)
}

#[derive(Serialize, Deserialize)]
pub struct RemoveFromChannel {
    pub bot: String,
    pub channel: String,
    pub user: String,
}

impl RemoveFromChannel {
    /// Serialize the body for the Slack API request
    pub fn body(&self) -> Result<String, String> {
        #[derive(Serialize)]
        struct RemoveFromChannelBody<'a> {
            channel: &'a str,
            user: &'a str,
        }

        let body = RemoveFromChannelBody {
            channel: &self.channel,
            user: &self.user,
        };

        serde_json::to_string(&body).map_err(|e| format!("Failed to serialize body: {}", e))
    }
}

/// Remove a user from a Slack channel
/// - `bot`: bot to use for removing the user
/// - `channel`: ID of the channel to remove the user from
/// - `user`: ID of the user to remove
pub fn remove_from_channel(bot: &str, channel: &str, user: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(slack, remove_from_channel);
    }

    let params = serde_json::to_string(&RemoveFromChannel {
        bot: bot.to_string(),
        channel: channel.to_string(),
        user: user.to_string(),
    })
    .unwrap();

    let res = unsafe { slack_remove_from_channel(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

#[derive(Serialize)]
struct SlackMessageWithOptions {
    channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocks: Option<serde_json::Value>,
    attachments: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    unfurl_links: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unfurl_media: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<serde_json::Value>,
}

/// Optional settings for [`post_message_with_options`].
#[derive(Default)]
pub struct PostMessageOptions {
    /// Whether Slack should unfurl links in the message.
    pub unfurl_links: Option<bool>,
    /// Whether Slack should unfurl media in the message.
    pub unfurl_media: Option<bool>,
    /// Message metadata JSON: `{"event_type": "...", "event_payload": {...}}`.
    /// Metadata is invisible to users but is returned by conversations.history,
    /// letting rules correlate posted messages back to the event that generated
    /// them (e.g. to recover the `ts` of a message delivered via scheduling).
    pub metadata: Option<String>,
}

/// Response from a `post_message` call. On success the message posted
/// immediately and `ts` is set. If the caller opted in to rate-limit handling
/// and the channel was rate limited, `ok` is false and `error` is
/// `"ratelimited"` — the caller should then schedule the message itself via
/// [`schedule_message`].
#[derive(Serialize, Deserialize)]
pub struct SlackDeliveryResponse {
    pub ok: bool,
    #[serde(default)]
    pub channel: Option<String>,
    /// Set when the message posted immediately.
    #[serde(default)]
    pub ts: Option<String>,
    /// Slack error code when `ok` is false (e.g. `"ratelimited"`).
    #[serde(default)]
    pub error: Option<String>,
}

impl SlackDeliveryResponse {
    /// Whether the post was rejected because the channel is rate limited, i.e.
    /// the caller should schedule the message for later delivery instead.
    pub fn rate_limited(&self) -> bool {
        !self.ok && self.error.as_deref() == Some("ratelimited")
    }
}

/// Post a Slack message with Block Kit blocks, attachments and optional
/// settings (unfurl behavior, message metadata). Returns a
/// [`SlackDeliveryResponse`]; callers that set `schedule_on_ratelimit` should
/// check [`SlackDeliveryResponse::rate_limited`] and, when true, deliver the
/// message themselves via [`schedule_message`] — the runtime does not schedule
/// automatically, it only reports the rate limit.
/// - `bot`: configured bot name
/// - `channel`: channel ID to post to
/// - `blocks`: Block Kit JSON array (may be empty)
/// - `attachments`: attachments JSON array
/// - `options`: see [`PostMessageOptions`]
pub fn post_message_with_options(
    bot: &str,
    channel: &str,
    blocks: &str,
    attachments: &str,
    options: PostMessageOptions,
) -> Result<SlackDeliveryResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, post_message);
    }

    let metadata = match options.metadata {
        Some(m) => Some(
            serde_json::from_str(&m).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?,
        ),
        None => None,
    };

    let message = SlackMessageWithOptions {
        channel: channel.to_owned(),
        blocks: optional_blocks(blocks)?,
        attachments: serde_json::from_str(attachments)
            .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?,
        unfurl_links: options.unfurl_links,
        unfurl_media: options.unfurl_media,
        metadata,
    };

    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    // Opt in to the runtime's scheduleMessage fallback on rate limit. Only this
    // helper sets the flag, so existing post helpers keep the historical 429
    // behavior and never see a scheduled-delivery response.
    #[derive(Serialize)]
    struct PostMessageScheduled {
        bot: String,
        body: String,
        schedule_on_ratelimit: bool,
    }

    let params = serde_json::to_string(&PostMessageScheduled {
        bot: bot.to_string(),
        body: serde_json::to_string(&message).unwrap(),
        schedule_on_ratelimit: true,
    })
    .unwrap();

    let res = unsafe {
        slack_post_message(
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
    let res = String::from_utf8(return_buffer).unwrap();

    serde_json::from_str(&res).map_err(|_| PlaidFunctionError::Unknown)
}

/// Data sent to the runtime to schedule a message (chat.scheduleMessage). The
/// `body` is the fully rendered Slack payload including `post_at`.
#[derive(Serialize, Deserialize)]
pub struct ScheduleMessage {
    pub bot: String,
    pub body: String,
}

/// Response from chat.scheduleMessage.
#[derive(Serialize, Deserialize)]
pub struct ScheduleMessageResponse {
    pub ok: bool,
    #[serde(default)]
    pub channel: Option<String>,
    /// Set on success; identifies the scheduled message for later deletion.
    #[serde(default)]
    pub scheduled_message_id: Option<String>,
    /// Unix timestamp the message is scheduled to post at.
    #[serde(default)]
    pub post_at: Option<u64>,
    /// Slack error code when `ok` is false.
    #[serde(default)]
    pub error: Option<String>,
}

impl ScheduleMessageResponse {
    /// Whether scheduling was rejected because this 5-minute window already
    /// holds the maximum scheduled messages for the channel
    /// (`restricted_too_many`), or the target time slipped into the past
    /// (`time_in_past`). Either way the caller should retry with a later
    /// `post_at`.
    pub fn window_full(&self) -> bool {
        !self.ok
            && matches!(
                self.error.as_deref(),
                Some("restricted_too_many") | Some("time_in_past")
            )
    }
}

#[derive(Serialize)]
struct ScheduledMessageBody {
    channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocks: Option<serde_json::Value>,
    attachments: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    unfurl_links: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unfurl_media: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<serde_json::Value>,
    post_at: u64,
}

/// Schedule a Block Kit message for future delivery (chat.scheduleMessage).
/// Used to deliver a message that would otherwise be dropped when a channel is
/// rate limited (see [`SlackDeliveryResponse::rate_limited`]). The runtime just
/// relays the call — the caller owns the retry/window policy and inspects
/// [`ScheduleMessageResponse::window_full`] to pick another `post_at`.
/// - `post_at`: Unix timestamp (seconds) to deliver at; must be in the future.
pub fn schedule_message(
    bot: &str,
    channel: &str,
    blocks: &str,
    attachments: &str,
    post_at: u64,
    options: PostMessageOptions,
) -> Result<ScheduleMessageResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, schedule_message);
    }

    let metadata = match options.metadata {
        Some(m) => Some(
            serde_json::from_str(&m).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?,
        ),
        None => None,
    };

    let message = ScheduledMessageBody {
        channel: channel.to_owned(),
        blocks: optional_blocks(blocks)?,
        attachments: serde_json::from_str(attachments)
            .map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?,
        unfurl_links: options.unfurl_links,
        unfurl_media: options.unfurl_media,
        metadata,
        post_at,
    };

    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&ScheduleMessage {
        bot: bot.to_string(),
        body: serde_json::to_string(&message).unwrap(),
    })
    .unwrap();

    let res = unsafe {
        slack_schedule_message(
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
    let res = String::from_utf8(return_buffer).unwrap();

    serde_json::from_str(&res).map_err(|_| PlaidFunctionError::Unknown)
}

/// Data to be sent to the runtime for deleting a scheduled message
#[derive(Serialize, Deserialize)]
pub struct DeleteScheduledMessage {
    /// Bot to use for deleting the scheduled message
    pub bot: String,
    /// ID of the channel the message is scheduled to post to
    pub channel: String,
    /// ID returned by chat.scheduleMessage
    pub scheduled_message_id: String,
}

impl DeleteScheduledMessage {
    /// Serialize the body for the Slack API request
    pub fn body(&self) -> Result<String, String> {
        #[derive(Serialize)]
        struct DeleteScheduledMessageBody<'a> {
            channel: &'a str,
            scheduled_message_id: &'a str,
        }

        let body = DeleteScheduledMessageBody {
            channel: &self.channel,
            scheduled_message_id: &self.scheduled_message_id,
        };

        serde_json::to_string(&body).map_err(|e| format!("Failed to serialize body: {}", e))
    }
}

/// Response from chat.deleteScheduledMessage. When `ok` is false, `error`
/// carries the Slack error code; `invalid_scheduled_message_id` means the
/// message already posted (or never existed) and can no longer be deleted.
#[derive(Serialize, Deserialize)]
pub struct DeleteScheduledMessageResponse {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
}

impl DeleteScheduledMessageResponse {
    /// Whether the delete failed because the message already posted
    /// (or the ID was never valid).
    pub fn already_posted(&self) -> bool {
        !self.ok && self.error.as_deref() == Some("invalid_scheduled_message_id")
    }
}

/// Delete a scheduled message before it posts (chat.deleteScheduledMessage).
/// - `bot`: configured bot name
/// - `channel`: ID of the channel the message is scheduled to post to
/// - `scheduled_message_id`: ID returned when the message was scheduled
///
/// Returns `ok: false` with the Slack error code rather than an `Err` for
/// API-level failures so callers can handle `invalid_scheduled_message_id`
/// (the message already posted) distinctly.
pub fn delete_scheduled_message(
    bot: &str,
    channel: &str,
    scheduled_message_id: &str,
) -> Result<DeleteScheduledMessageResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, delete_scheduled_message);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&DeleteScheduledMessage {
        bot: bot.to_string(),
        channel: channel.to_string(),
        scheduled_message_id: scheduled_message_id.to_string(),
    })
    .unwrap();

    let res = unsafe {
        slack_delete_scheduled_message(
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

    serde_json::from_str(&res).map_err(|_| PlaidFunctionError::Unknown)
}

/// Data to be sent to the runtime for fetching channel history
#[derive(Serialize, Deserialize)]
pub struct ConversationsHistory {
    /// Bot to use for fetching history
    pub bot: String,
    /// ID of the channel to fetch history for
    pub channel: String,
    /// Maximum number of messages to return
    #[serde(default)]
    pub limit: Option<u32>,
    /// Only include messages after this Slack timestamp
    #[serde(default)]
    pub oldest: Option<String>,
}

/// Message metadata as returned by conversations.history
#[derive(Serialize, Deserialize)]
pub struct MessageMetadata {
    pub event_type: String,
    #[serde(default)]
    pub event_payload: serde_json::Value,
}

/// A single message in a conversations.history response. Only the fields
/// needed for correlating messages are deserialized.
#[derive(Serialize, Deserialize)]
pub struct HistoryMessage {
    pub ts: String,
    #[serde(default)]
    pub metadata: Option<MessageMetadata>,
}

/// Response from conversations.history
#[derive(Serialize, Deserialize)]
pub struct ConversationsHistoryResponse {
    pub ok: bool,
    #[serde(default)]
    pub messages: Vec<HistoryMessage>,
}

/// Fetch recent message history for a channel (conversations.history),
/// including message metadata. Requires the bot to have the
/// `channels:history` scope (or `groups:history` for private channels).
/// - `bot`: configured bot name
/// - `channel`: ID of the channel to fetch history for
/// - `limit`: maximum number of messages to return
/// - `oldest`: only include messages after this Slack timestamp
pub fn conversations_history(
    bot: &str,
    channel: &str,
    limit: Option<u32>,
    oldest: Option<&str>,
) -> Result<ConversationsHistoryResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, conversations_history);
    }
    // History responses carry full message payloads so they can be large.
    const RETURN_BUFFER_SIZE: usize = 256 * 1024; // 256 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&ConversationsHistory {
        bot: bot.to_string(),
        channel: channel.to_string(),
        limit,
        oldest: oldest.map(|o| o.to_string()),
    })
    .unwrap();

    let res = unsafe {
        slack_conversations_history(
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

    serde_json::from_str(&res).map_err(|_| PlaidFunctionError::Unknown)
}
