use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

fn main(_log: String, _source: LogSource) -> Result<(), i32> {
    // Wait 1 second to simulate execution latency
    let now = plaid::get_time();
    while plaid::get_time() < now + 1 {}

    make_named_request("test-response", "OK", HashMap::new()).unwrap();

    Ok(())
}
