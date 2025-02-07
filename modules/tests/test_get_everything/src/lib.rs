use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source_and_response, messages::LogSource, network::make_named_request, plaid,
};

entrypoint_with_source_and_response!();

fn handle_post() -> Result<Option<String>, i32> {
    // get_secrets
    let secret = plaid::get_secrets("my_secret").unwrap();
    assert_eq!(secret.as_str(), "verySecureSecret");

    // get_headers
    let header = plaid::get_headers("Authorization").unwrap();
    assert_eq!(header.as_str(), "Some Authorization Header");

    // get header whose name clashes with a secret
    let header = plaid::get_headers("my_secret").unwrap();
    assert_eq!(header.as_str(), "Secret from a header");

    // get accessory data which was set as a mix of universal and per-rule accessory data
    let value_1 = plaid::get_accessory_data("key_1").unwrap();
    assert_eq!(value_1, "value_1_new");
    let value_2 = plaid::get_accessory_data("key_2").unwrap();
    assert_eq!(value_2, "value_2");
    let value_3 = plaid::get_accessory_data("key_3").unwrap();
    assert_eq!(value_3, "value_3");
    let value_4 = plaid::get_accessory_data("key_4").unwrap();
    assert_eq!(value_4, "value_4");

    // All good
    make_named_request("test-response", "OK", HashMap::new()).unwrap();
    Ok(None)
}

fn handle_get() -> Result<Option<String>, i32> {
    // get_secrets
    let secret = plaid::get_secrets("my_secret").unwrap();
    assert_eq!(secret.as_str(), "verySecureSecret");

    // get_query_params
    let param = plaid::get_query_params("q").unwrap();
    assert_eq!(param.as_str(), "queryParameter");

    // get query param whose name clashes with a secret
    let param = plaid::get_query_params("my_secret").unwrap();
    assert_eq!(param.as_str(), "secretFromQueryParam");

    // All good
    make_named_request("test-response", "OK", HashMap::new()).unwrap();
    Ok(Some("OK".to_string()))
}

fn main(_log: String, source: LogSource) -> Result<Option<String>, i32> {
    match source {
        LogSource::WebhookPost(_) => handle_post(),
        LogSource::WebhookGet(_) => handle_get(),
        _ => panic!(),
    }
}
