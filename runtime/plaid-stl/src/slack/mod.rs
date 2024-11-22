use std::collections::HashMap;

use serde::Serialize;

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
        return Err(res);
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
        return Err(res);
    }

    Ok(())
}

pub fn post_message(bot: &str, channel: &str, text: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(slack, post_message);
    }

    let message = SlackMessage {
        channel: channel.to_owned(),
        text: text.to_owned(),
    };

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("bot", bot.to_string());
    params.insert("body", serde_json::to_string(&message).unwrap());

    let params = serde_json::to_string(&params).unwrap();

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

    let mut params: HashMap<&'static str, String> = HashMap::new();
    params.insert("bot", bot.to_string());
    params.insert("body", serde_json::to_string(&message).unwrap());

    let params = serde_json::to_string(&params).unwrap();

    let res = unsafe { slack_post_message(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

pub fn views_open(bot: &str, view: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(slack, views_open);
    }

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("bot", bot);
    params.insert("body", view);

    let params = serde_json::to_string(&params).unwrap();

    let res = unsafe { slack_views_open(params.as_ptr(), params.len()) };

    if res < 0 {
        return Err(res.into());
    }

    Ok(())
}

/// Get a Slack user's ID from their email address
pub fn get_id_from_email(bot: &str, email: &str) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(slack, get_id_from_email);
    }
    const RETURN_BUFFER_SIZE: usize = 32 * 1024; // 32 KiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let mut params: HashMap<&'static str, &str> = HashMap::new();
    params.insert("bot", bot);
    params.insert("email", email);

    let params = serde_json::to_string(&params).unwrap();

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
