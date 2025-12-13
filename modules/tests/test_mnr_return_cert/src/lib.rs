use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

fn main(log: String, _source: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("testing test_mnr_return_cert: [{log}]"));

    let output = make_named_request("test_mnr_return_cert", "", HashMap::new()).unwrap();

    if let Some(cert) = output.cert {
        plaid::print_debug_string(&format!("Received cert: {cert}"));

        // If we are here, then everything worked fine (no unwraps or early returns), so we send an OK
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    } else {
        plaid::print_debug_string(&format!("output.cert was empty. something is wrong",));
    }

    Ok(())
}
