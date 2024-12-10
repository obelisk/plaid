use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::{make_named_request, make_named_request_with_headers}};

entrypoint_with_source!();

fn main(_log: String, _source: LogSource) -> Result<(), i32> {
    // MNR that we cannot do
    match make_named_request("google_test", "", HashMap::new()) {
        Ok(_) => panic!(),
        Err(_) => (),
    };

    // Simple MNR
    make_named_request("test-response-mnr", "OK", HashMap::new()).unwrap();

    // MNR with variables
    make_named_request("test-response-mnr-vars", "OK", HashMap::from([("variable".to_string(), "my_variable".to_string())])).unwrap();

    // MNR with headers
    make_named_request_with_headers("test-response-mnr-headers", "OK", HashMap::new(), HashMap::from([("second_header".to_string(), "second_value".to_string())])).unwrap();

    Ok(())
}
