use plaid_stl::{entrypoint_with_source, messages::LogSource, plaid};
use regex::Regex;

entrypoint_with_source!();

// Test that SSHCerts can be linked and used correctly to verify
// SSH formatted signatures. We use unwraps judiciously because nothing
// should fail here and if it does, then it needs investigation.
fn main(log: String, _source: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Testing regex With Log: [{log}]"));

    let regex = Regex::new(
        r#"(?:[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*|"(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21\x23-\x5b\x5d-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])*")@(?:(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?|\[(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?|[a-z0-9-]*[a-z0-9]:(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21-\x5a\x53-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])+)\])"#,
    ).unwrap();

    // Test the regex against the passed in log
    let is_match = regex.is_match(&log);
    plaid::print_debug_string(&format!("Regex match result: {is_match}"));
    Ok(())
}
