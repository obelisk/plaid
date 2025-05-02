use std::collections::HashMap;

use plaid_stl::network::make_named_request;
use plaid_stl::slack::post_text_to_webhook;
use plaid_stl::{entrypoint_with_source, messages::LogSource, plaid};

entrypoint_with_source!();

fn main(log: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Testing slack APIs with log: {log}"));

    if let Err(_) = post_text_to_webhook("test_webhook", "Testing this makes it to slack") {
        plaid::print_debug_string("Failed to post to slack");
        panic!("Couldn't post to slack")
    }

    make_named_request("test-response", "OK", HashMap::new()).unwrap();

    Ok(())
}
