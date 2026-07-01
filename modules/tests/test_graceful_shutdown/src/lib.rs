use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

fn main(log: String, source: LogSource) -> Result<(), i32> {
    // Wait 1 second to simulate execution latency
    let now = plaid::get_time();
    while plaid::get_time() < now + 1 {}

    let output = match source {
        LogSource::WebhookPost(_) => {
            // Send a 0 delay logback - we use this to verify that all log backs get processed when
            // scheduled during a shutdown
            let _ = plaid::log_back("test_graceful_shutdown", log.as_bytes(), 0);

            format!("webhook_{log}")
        }
        LogSource::Logback(_) => {
            format!("logback_{log}")
        }
        _ => return Err(-1),
    };

    make_named_request("test-response", &output, HashMap::new()).unwrap();

    Ok(())
}
