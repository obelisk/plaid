use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

fn main(log: String, _source: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("testing test_mnr_return_certs: [{log}]"));

    let output = make_named_request("test_mnr_return_certs", "", HashMap::new()).unwrap();

    plaid::print_debug_string(&format!(
        "output {}",
        serde_json::to_string_pretty(&output).unwrap()
    ));

    Ok(())
}
