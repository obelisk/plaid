use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source_and_response, messages::LogSource, network::make_named_request, plaid,
};

entrypoint_with_source_and_response!();

const CACHE_KEY: &str = "cache_key";

/// When a GET is received, we read a value from the cache and send it to the request handler.
fn handle_get() -> Result<Option<String>, i32> {
    let mut value = plaid::cache::get(CACHE_KEY).unwrap();
    if value.is_empty() {
        value = "0".to_string();
    }
    make_named_request("test-response", &value, HashMap::new()).unwrap();
    plaid::log_back("test_logback", b"", 3).unwrap();
    Ok(Some("OK".to_string()))
}

/// When a log back is received, we simply put a value in the cache.
/// Then we try another log back, which should fail since our budget was only 1.
fn handle_logback() -> Result<Option<String>, i32> {
    plaid::cache::insert(CACHE_KEY, "1").unwrap();

    /*
    This is meant to test the case where the budget is limited.
    With an insufficient budget, this call should probably fail, so that the
    rule knows the log back will not happen. In reality, the current behavior
    is that the call will succeed, but the runtime may not honour the request.
    
    match plaid::log_back("test_logback", b"something", 0) {
        Ok(_) => { panic!("this should not happen") },
        Err(_) => {
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
    };
    */
    
    Ok(Some("OK".to_string()))
}

fn main(_log: String, source: LogSource) -> Result<Option<String>, i32> {
    match source {
        LogSource::WebhookGet(_) => handle_get(),
        LogSource::Logback(_) => handle_logback(),
        _ => panic!(),
    }
}
