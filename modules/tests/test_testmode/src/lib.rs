use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network, plaid, PlaidFunctionError};

entrypoint_with_source!();

fn main(log: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Testing testmode With Log: [{log}]"));

    // This should work because the configuration allows this particular request
    match network::make_named_request("testmode_allow", "OK", HashMap::new()) {
        Ok(_) => {
            plaid::print_debug_string("Testmode allowed request succeeded");
        }
        Err(e) => {
            plaid::print_debug_string(&format!("Testmode allowed request failed: [{e}]"));
            panic!("Testmode allowed request failed");
        }
    }

    // This should not work because the configuration denies this particular request
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

    // This should fail because simple_json_post_request is not allowed in test mode
    match network::simple_json_post_request("https://captive.apple.com", "{}", None) {
        Err(PlaidFunctionError::TestMode) => {
            plaid::print_debug_string("Testmode denied request succeeded");
        }
        Err(e) => {
            plaid::print_debug_string(&format!("Testmode denied request failed: [{e}]"));
            panic!("Testmode denied request failed");
        }
        Ok(_) => {
            panic!("Testmode request that should have failed succeeded");
        }
    }
    Ok(())
}
