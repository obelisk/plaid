use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source,
    messages::LogSource,
    network::make_named_request,
    plaid::{self, storage::Item},
};

entrypoint_with_source!();

const SHARED_DB: &str = "shared_db_1";
const RULE_NAME: &str = "test_shared_db_rule_2";

const BATCH_INSERT_SIZE: usize = 3;

fn main(log: String, _: LogSource) -> Result<(), i32> {
    let batch_insert_items = generate_batch_insert_items();

    // Depending on the value of "log", we do different things
    match log.as_str() {
        "write and check" => {
            plaid::print_debug_string(&format!("[{RULE_NAME}] Writing to DB..."));
            plaid::storage::insert_shared(SHARED_DB, "my_key", &vec![0u8, 1u8]).unwrap();

            plaid::print_debug_string(&format!("[{RULE_NAME}] Reading from DB..."));
            let r = plaid::storage::get_shared(SHARED_DB, "my_key").unwrap();
            plaid::print_debug_string(&format!("[{RULE_NAME}] Got {} bytes", r.len()));
            if r.len() != 2 {
                panic!()
            }

            plaid::print_debug_string(&format!("[{RULE_NAME}] Listing keys from DB..."));
            let x = plaid::storage::list_keys_shared(SHARED_DB, None::<String>).unwrap();
            plaid::print_debug_string(&format!("[{RULE_NAME}] Got {} keys", x.len()));
            if x.len() != 1 {
                panic!()
            }
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        "delete and check" => {
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
        "fill up the db" => {
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
        "write to full db" => {
            plaid::print_debug_string(&format!(
                "[{RULE_NAME}] Writing to a full shared DB, should fail..."
            ));
            match plaid::storage::insert_shared(SHARED_DB, "another_key", &vec![0u8]) {
                Ok(_) => panic!("Single insert on a full DB should have failed"),
                Err(_) => {
                    plaid::print_debug_string(&format!(
                        "[{RULE_NAME}] Failed as expected on single item insert"
                    ));
                }
            }
            match plaid::storage::insert_batch_shared(SHARED_DB, &batch_insert_items) {
                Ok(_) => panic!("Batch insert on a full DB should have failed"),
                Err(_) => {
                    plaid::print_debug_string(&format!(
                        "[{RULE_NAME}] Failed as expected on batch insert"
                    ));
                }
            }

            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        "write to non-existing db" => {
            plaid::print_debug_string(&format!(
                "[{RULE_NAME}] Writing to a non-existing shared DB, should fail..."
            ));
            match plaid::storage::insert_shared("this_does_not_exist", "some_key", &vec![0u8]) {
                Ok(_) => panic!("Single insert to non-existant DB should have failed"),
                Err(_) => {
                    plaid::print_debug_string(&format!(
                        "[{RULE_NAME}] Failed as expected on single item write to non-existent DB"
                    ));
                }
            }
            match plaid::storage::insert_batch_shared("this_does_not_exist", &batch_insert_items) {
                Ok(_) => panic!("Batch insert to non-existant DB should have failed"),
                Err(_) => {
                    plaid::print_debug_string(&format!(
                        "[{RULE_NAME}] Failed as expected on batch item write to non-existent DB"
                    ));
                }
            }
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        "insert batch and check" => {
            plaid::print_debug_string(&format!("[{RULE_NAME}] Writing 3 items to DB..."));

            plaid::storage::insert_batch_shared(SHARED_DB, &batch_insert_items).unwrap();

            for item in batch_insert_items {
                let returned_val = plaid::storage::get_shared(SHARED_DB, &item.key).unwrap();

                if returned_val != item.value {
                    panic!(
                        "Returned value does not match expected value for key: {}",
                        item.key
                    )
                }
            }
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
        }
        _ => panic!("Got an unexpected log"),
    }

    Ok(())
}

fn generate_batch_insert_items() -> Vec<Item> {
    let mut items = vec![];
    for i in 0..BATCH_INSERT_SIZE {
        let key = format!("key{i}");
        let value = format!("value{i}");

        let item = Item {
            key,
            value: value.as_bytes().to_vec(),
        };

        items.push(item)
    }

    items
}
