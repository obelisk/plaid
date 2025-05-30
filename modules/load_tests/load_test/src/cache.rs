use plaid_stl::plaid::{cache, random::fetch_random_bytes};

pub fn load_test_cache() {
    // Choose a random number of repetitions, between 0 and 99.
    let repetitions = fetch_random_bytes(1).unwrap()[0] % 100;

    for i in 0..repetitions {
        // Write something to the cache...
        cache::insert(&format!("key{i}"), &format!("value{i}")).unwrap();

        // ... and read it
        cache::get(&format!("key{i}")).unwrap();

        // Also read something which is not there
        cache::get("does_not_exist").unwrap();
    }
}
