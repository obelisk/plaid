use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};
use serde::Deserialize;

entrypoint_with_source!();

#[derive(Deserialize)]
struct Log {
    #[serde(rename = "type")]
    type_: String,
}

fn main(log: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Received log {log}"));

    let log = serde_json::from_str::<Log>(&log).unwrap();

    make_named_request("test-response", &log.type_, HashMap::new()).unwrap();

    Ok(())
}
