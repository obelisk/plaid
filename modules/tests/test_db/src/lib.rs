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
        "list_keys" => match parts[1] {
            "all" => {
                // list_keys:all:key1|key2|key3
                let keys = plaid::storage::list_keys(None::<String>).unwrap();
                if are_vectors_equal(parts[2], keys) {
                    make_named_request("test-response", "OK", HashMap::new()).unwrap();
                } else {
                    panic!();
                }
            }
            "prefix" => {
                // list_keys:prefix:the_prefix:key1|key2|key3
                let keys = plaid::storage::list_keys(Some(parts[2])).unwrap();
                if are_vectors_equal(parts[3], keys) {
                    make_named_request("test-response", "OK", HashMap::new()).unwrap();
                } else {
                    panic!();
                }
            }
            _ => panic!(),
        },
        "insert_check_returned_data" => {
            let key = parts[1];
            // The data we are overwriting
            let initial_data = parts[2].as_bytes();
            let new_data = vec![42u8];
            let res = plaid::storage::insert(key, &new_data).unwrap();
            if res != initial_data {
                panic!();
            }
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        "delete_check_returned_data" => {
            let key = parts[1];
            // The data we are deleting
            let initial_data = parts[2].as_bytes();
            let res = plaid::storage::delete(key).unwrap();
            if res != initial_data {
                panic!();
            }
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
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

/// Compare two vectors, one of which is given encoded in pipe-delimited format.
/// The elements are not necessarily ordered the same way.
///
/// `assert!(are_vectors_equal("second|first|third", vec!["first".to_string(), "second".to_string(), "third".to_string()]))`
fn are_vectors_equal(pipe_delimited: &str, v: Vec<String>) -> bool {
    let mut v1 = pipe_delimited
        .split("|")
        .map(|v| v.to_string())
        .collect::<Vec<String>>();
    v1.sort();
    let mut v2 = v.clone();
    v2.sort();
    v1 == v2
}
