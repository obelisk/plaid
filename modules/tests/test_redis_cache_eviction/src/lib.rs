use std::collections::HashMap;

use plaid_stl::{
    entrypoint_with_source,
    messages::LogSource,
    network::make_named_request,
    plaid::{self, cache},
};

entrypoint_with_source!();

const MAX_CAPACITY: u32 = 50;

fn main(_: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Filling the cache..."));
    for i in 0..MAX_CAPACITY {
        cache::insert(&i.to_string(), "x").unwrap();
    }
    plaid::print_debug_string(&format!("The cache has been filled."));

    // Insert one more item, which should succeed but trigger the eviction of another one
    cache::insert(&(MAX_CAPACITY + 1).to_string(), "x").unwrap();

    // Now, if we try to get all the initial values, one (and exactly one) should be missing
    let mut missing = 0;
    for i in 0..MAX_CAPACITY {
        let r = cache::get(&i.to_string()).unwrap();
        if r.is_empty() {
            missing += 1;
        }
    }
    if missing != 1 {
        panic!("Missing is {missing} instead of 1");
    }

    // For good measure, check that the last item inserted is retrievable
    let r = cache::get(&(MAX_CAPACITY + 1).to_string()).unwrap();
    if r.is_empty() {
        panic!("Could not retrieve last inserted item");
    }

    // If we are here, then everything is fine: send OK
    make_named_request("test-response", "OK", HashMap::new()).unwrap();
    Ok(())
}
