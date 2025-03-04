use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

const SHARED_DB: &str = "shared_db_1";
const RULE_NAME: &str = "test_shared_db_rule_2";

fn main(log: String, _: LogSource) -> Result<(), i32> {
    // Depending on the value of "log", we do different things
    match log.as_str() {
        "1" => {
            plaid::print_debug_string(&format!("[{RULE_NAME}] Writing to DB..."));
            plaid::storage::insert_shared(SHARED_DB, "my_key", &vec![0u8, 1u8]).unwrap();

            plaid::print_debug_string(&format!("[{RULE_NAME}] Reading from DB..."));
            let r = plaid::storage::get_shared(SHARED_DB, "my_key").unwrap();
            plaid::print_debug_string(&format!("[{RULE_NAME}] Got {} bytes", r.len()));
            if r.len() != 2 {
                panic!()
            }
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        "2" => {
            plaid::print_debug_string(&format!("[{RULE_NAME}] Deleting from DB..."));
            plaid::storage::delete_shared(SHARED_DB, "my_key").unwrap();

            plaid::print_debug_string(&format!("[{RULE_NAME}] Reading from DB..."));
            let r = plaid::storage::get_shared(SHARED_DB, "my_key").unwrap();
            plaid::print_debug_string(&format!("[{RULE_NAME}] Got {} bytes", r.len()));
            if r.len() != 0 {
                panic!()
            }
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        "3" => {
            plaid::print_debug_string(&format!("[{RULE_NAME}] Filling up the shared DB..."));
            plaid::storage::insert_shared(SHARED_DB, "my_key", &vec![0u8; 44]).unwrap();

            plaid::print_debug_string(&format!("[{RULE_NAME}] Reading from DB..."));
            let r = plaid::storage::get_shared(SHARED_DB, "my_key").unwrap();
            plaid::print_debug_string(&format!(
                "[{RULE_NAME}] Got {} bytes (+ {} bytes for the key)",
                r.len(),
                "my_key".as_bytes().len()
            ));
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        "4" => {
            plaid::print_debug_string(&format!(
                "[{RULE_NAME}] Writing to a full shared DB, should fail..."
            ));
            match plaid::storage::insert_shared(SHARED_DB, "another_key", &vec![0u8]) {
                Ok(_) => panic!("This should have failed"),
                Err(_) => {
                    plaid::print_debug_string(&format!("[{RULE_NAME}] Failed as expected"));
                }
            }

            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        _ => panic!("Got an unexpected log"),
    }

    Ok(())
}
