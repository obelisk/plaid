use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source,
    messages::LogSource,
    network::{make_named_request, retrieve_tls_certificate_with_sni, TlsCertWithSniRequest},
    plaid::{self, print_debug_string},
};

entrypoint_with_source!();

fn main(log: String, _source: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("testing test_tls_cert_with_sni: [{log}]"));

    let destination = "104.154.89.105"; // expired.badssl.com
    let sni = "expired.badssl.com";

    let cert = retrieve_tls_certificate_with_sni(&TlsCertWithSniRequest {
        domain: destination.to_string(),
        sni: sni.to_string(),
    })
    .unwrap();

    print_debug_string(&format!("Retrieved TLS certificate: [{cert}]"));

    // If we are here, then everything worked fine (no unwraps or early returns), so we send an OK
    make_named_request("test-response", "OK", HashMap::new()).unwrap();

    Ok(())
}
