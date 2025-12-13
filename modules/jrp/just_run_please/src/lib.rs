use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network, plaid};

entrypoint_with_source!();

fn main(data: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!(
        "I just ran please and thank you. Here's the data: {data}"
    ));

    let result = network::make_named_request("testmode_allow", "", HashMap::new())?;

    let cert = result.cert.unwrap();

    plaid::print_debug_string(&format!("Received cert: {cert}"));
    Ok(())
}
