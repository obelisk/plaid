use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source_and_response, messages::LogSource, network::make_named_request, plaid,
};

entrypoint_with_source_and_response!();

const KEY: &str = "my_key";

/// When a GET is received, return a value read from the DB
fn handle_get() -> Result<Option<String>, i32> {
    let key = plaid::get_accessory_data_by_name("key").unwrap();
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
    Ok(Some("OK".to_string()))
}

/// When a POST is received, write the received value to the DB, under a predefined key
fn handle_post(log: &str) -> Result<Option<String>, i32> {
    plaid::storage::insert(KEY, log.as_bytes()).unwrap();
    Ok(Some("OK".to_string()))
}

fn main(log: String, source: LogSource) -> Result<Option<String>, i32> {
    match source {
        LogSource::WebhookGet(_) => handle_get(),
        LogSource::WebhookPost(_) => handle_post(&log),
        _ => panic!(),
    }
}
