use base64::prelude::*;
use std::collections::HashMap;

use plaid_stl::{
    entrypoint_vec_with_source, messages::LogSource, network::make_named_request, plaid,
};

entrypoint_vec_with_source!();

fn main(log: Vec<u8>, _: LogSource) -> Result<(), i32> {
    let log_len = log.len();
    let log_b64 = BASE64_STANDARD.encode(log);
    plaid::print_debug_string(&format!(
        "The rule received {log_len} raw bytes and this is their base64 encoding: {log_b64}"
    ));

    make_named_request("test-response", &log_len.to_string(), HashMap::new()).unwrap();
    Ok(())
}
