use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, embed_plaid_profile, messages::LogSource, network, plaid};


embed_plaid_profile!(SECURITY, "../security-profiles/permissive.json");
entrypoint_with_source!();

fn main(data: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!(
        "I just ran please and thank you. Here's the data: {data}"
    ));

    let result = network::make_named_request("testmode_allow", "", HashMap::new())?;

    let cert = result.cert.unwrap();

    plaid::print_debug_string(&format!("Received cert: {cert}"));

    plaid::set_error_context(
        "Setting some error context which gets logged as Severity::Info in the runtime",
    );
    Ok(())
}
