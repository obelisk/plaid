use std::collections::HashMap;

use plaid_stl::{entrypoint_with_source, messages::LogSource, network::make_named_request, plaid};

entrypoint_with_source!();

const REPS: u16 = 50;

fn main(_: String, _: LogSource) -> Result<(), i32> {
    for num_bytes in 1..=100 {
        for _ in 0..REPS {
            let bytes = plaid::random::fetch_random_bytes(num_bytes).unwrap();
            if bytes.len() as u16 != num_bytes {
                panic!()
            }
        }
    }

    make_named_request("test-response", "OK", HashMap::new()).unwrap();

    Ok(())
}
