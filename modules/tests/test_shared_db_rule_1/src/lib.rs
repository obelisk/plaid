use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

const SHARED_DB: &str = "shared_db_1";
const RULE_NAME: &str = "test_shared_db_rule_1";

fn main(log: String, _: LogSource) -> Result<(), i32> {
    // Depending on the value of "log", we do different things
    match log.as_str() {
        "1" => {
            plaid::print_debug_string(&format!("[{RULE_NAME}] Reading from DB..."));
            let r = plaid::storage::get_shared(SHARED_DB, "my_key").unwrap();
            plaid::print_debug_string(&format!("[{RULE_NAME}] Got {} bytes", r.len()));
            if r.len() != 0 {
                panic!()
            }
            plaid::print_debug_string(&format!(
                "[{RULE_NAME}] Writing to DB (which is not allowed)..."
            ));
            match plaid::storage::insert_shared(SHARED_DB, "my_key", &vec![0u8, 1u8]) {
                Ok(_) => panic!("This should have failed"),
                Err(_) => {}
            }
            plaid::print_debug_string(&format!("[{RULE_NAME}] Failed as expected"));
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        "2" => {
            // Meanwhile, another rule will have written 2 bytes to the shared DB
            plaid::print_debug_string(&format!("[{RULE_NAME}] Reading from DB..."));
            let r = plaid::storage::get_shared(SHARED_DB, "my_key").unwrap();
            plaid::print_debug_string(&format!("[{RULE_NAME}] Got {} bytes", r.len()));
            if r.len() != 2 {
                panic!()
            }
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        "3" => {
            // Meanwhile, another rule will have deleted the key from the shared DB
            plaid::print_debug_string(&format!("[{RULE_NAME}] Reading from DB..."));
            let r = plaid::storage::get_shared(SHARED_DB, "my_key").unwrap();
            plaid::print_debug_string(&format!("[{RULE_NAME}] Got {} bytes", r.len()));
            if r.len() != 0 {
                panic!()
            }
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        _ => panic!("Got an unexpected log"),
    }

    Ok(())
}
