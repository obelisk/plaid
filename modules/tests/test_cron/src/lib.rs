use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

fn main(_: String, _: LogSource) -> Result<(), i32> {
    let time = plaid::get_time();

    // This posts to its own MNR because this will running throughout the entire duration of the integration test.
    // To keep it from intefering with other rules' tests, we send all its output to a dedicated endpoint.
    make_named_request("test-cron", &time.to_string(), HashMap::new()).unwrap();

    Ok(())
}
