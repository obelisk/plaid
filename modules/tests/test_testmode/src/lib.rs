use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network, plaid, PlaidFunctionError};

entrypoint_with_source!();

fn main(log: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Testing testmode With Log: [{log}]"));

    match network::make_named_request("testmode_allow", "OK", HashMap::new()) {
        Ok(_) => {
            plaid::print_debug_string("Testmode allowed request succeeded");
        }
        Err(e) => {
            plaid::print_debug_string(&format!("Testmode allowed request failed: [{e}]"));
            panic!("Testmode allowed request failed");
        }
    }

    let _: Result<(), ()> = match network::make_named_request("testmode_deny", "OK", HashMap::new())
    {
        Err(PlaidFunctionError::TestMode) => Ok(()),
        Err(e) => {
            panic!("Testmode denied request did not fail with the right error: [{e}]");
        }
        Ok(_) => {
            panic!("Testmode request that should have failed succeeded");
        }
    };

    Ok(())
}
