mod cache;
mod regex;

use plaid_stl::{
    entrypoint_with_source_and_response,
    messages::LogSource,
    plaid::{get_time, print_debug_string, random::fetch_random_bytes},
};
use serde::{Deserialize, Serialize};

entrypoint_with_source_and_response!();

#[derive(Serialize, Deserialize)]
struct Log {
    input_for_regex: Option<String>,
    regex_to_use: Option<String>,
    get_time: Option<bool>,
    get_random_bytes: Option<u16>,
    use_cache: Option<bool>,
    post_slack: Option<bool>,
}

fn main(log: String, source: LogSource) -> Result<Option<String>, i32> {
    // Only accept GET
    match source {
        LogSource::WebhookGet(_) => (),
        LogSource::WebhookPost(_) => (),
        _ => panic!(),
    }

    let log = serde_json::from_str::<Log>(&log).unwrap();

    // This regex test is quite heavy, so we flip a coin:
    // sometimes we do it and sometimes we don't
    if fetch_random_bytes(1).unwrap()[0] % 2 == 0 {
        print_debug_string("Doing the regex test");
        regex::load_test_regex(log.input_for_regex, log.regex_to_use);
    } else {
        print_debug_string("Skipping the regex test");
    }

    if log.get_time.unwrap_or(false) {
        let _ = get_time();
    }

    if let Some(random_bytes) = log.get_random_bytes {
        let _ = fetch_random_bytes(random_bytes).unwrap();
    }

    if log.use_cache.unwrap_or(false) {
        cache::load_test_cache();
    }

    if log.post_slack.unwrap_or(false) {}

    Ok(Some("Done".to_string()))
}
