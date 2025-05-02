use std::collections::HashMap;

use plaid_stl::network::make_named_request;
use plaid_stl::slack::{self, post_message, post_text_to_webhook};
use plaid_stl::{entrypoint_with_source, messages::LogSource, plaid};

entrypoint_with_source!();

fn main(log: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Testing slack APIs with log: {log}"));

    if let Err(_) = post_text_to_webhook("test_webhook", "Testing this makes it to slack") {
        plaid::print_debug_string("Failed to post to slack");
        panic!("Couldn't post to slack")
    }

    make_named_request("test-response", "OK", HashMap::new()).unwrap();

    let user_id = slack::get_id_from_email("plaid-testing", "mitchell@confurious.io")
        .unwrap_or_else(|_| {
            plaid::print_debug_string("Failed to get user ID from email");
            panic!("Couldn't get user ID from email")
        });

    make_named_request("test-response", "OK", HashMap::new()).unwrap();

    if let Err(_) = post_message(
        "plaid-testing",
        &user_id,
        "Testing that this goes directly to obelisk",
    ) {
        plaid::print_debug_string("Failed to send Slack message");
        panic!("Couldn't send Slack message")
    }

    make_named_request("test-response", "OK", HashMap::new()).unwrap();

    Ok(())
}
