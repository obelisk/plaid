use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

/// When a POST is received, perform the requested action
fn handle_post(log: &str) -> Result<(), i32> {
    // The format of the log will be
    // action:key[:value]
    let parts: Vec<&str> = log.split(':').collect();
    match parts[0] {
        "get" => {
            let key = parts[1];
            let mut value = plaid::storage::get(&key).unwrap();
            if value.is_empty() {
                value = "Empty".as_bytes().to_vec();
            }
            make_named_request(
                "test-response",
                String::from_utf8(value).unwrap().as_str(),
                HashMap::new(),
            )
            .unwrap();
        }
        "insert" => {
            plaid::storage::insert(parts[1], parts[2].as_bytes()).unwrap();
        }
        "delete" => {
            plaid::storage::delete(parts[1]).unwrap();
        }
        _ => panic!(),
    }
    Ok(())
}

fn main(log: String, source: LogSource) -> Result<(), i32> {
    match source {
        LogSource::WebhookPost(_) => handle_post(&log),
        _ => panic!(),
    }
}
