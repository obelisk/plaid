use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

fn main(log: String, _source: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("testing test_mnr_return_certs: [{log}]"));

    // TODO: need another test where MNR is calling localhost with custom root CA
    let output = make_named_request("test_mnr_return_certs", "OK", HashMap::new()).unwrap();

    if let Some(certs) = output.certs {
        plaid::print_debug_string(&format!("cert chain len = {}", certs.len()));
        for (i, c) in certs.iter().enumerate() {
            plaid::print_debug_string(&format!("Cert {i}"));
            plaid::print_debug_string(&format!("{c}"));
        }

        // If we are here, then everything worked fine (no unwraps or early returns), so we send an OK
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    } else {
        plaid::print_debug_string(&format!("output.certs was empty. something is wrong",));
    }

    Ok(())
}
