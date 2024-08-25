use std::collections::HashMap;

use plaid_stl::{entrypoint, network::make_named_request, plaid};

entrypoint!();

fn main(log: String) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Testing Time With Log: [{log}]"));

    let time = plaid::get_time();

    make_named_request("test-response", &time.to_string(), HashMap::new()).unwrap();

    Ok(())
}
