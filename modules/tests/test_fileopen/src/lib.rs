use std::{collections::HashMap, fs::File};

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

const FILE_PATH: &str = "plaid/resources/plaid.toml";

fn main(_: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Trying to open {FILE_PATH}..."));

    // Open the file
    match File::open(FILE_PATH) {
        Ok(_) => panic!(),
        Err(e) => {
            plaid::print_debug_string(&format!(
                "I did not manage to open the file. This is good! Error: {e}"
            ));
            make_named_request("test-response", "OK", HashMap::new()).unwrap();
            Ok(())
        }
    }
}
