//! # Counter Example
//!
//! Demonstrates the difference between **persistent storage** and
//! **in-memory cache** by maintaining a counter in each.
//!
//! - **Storage** (`plaid::storage`) persists across restarts (backed by Sled).
//!   Values are `&[u8]` — you serialize/deserialize yourself.
//! - **Cache** (`plaid::cache`) is in-memory and ephemeral. It resets when plaid
//!   restarts. Values are `&str`.
//!
//! Each webhook call increments both counters and returns the current values.
//! Restart plaid to see the cache reset while storage persists.
//!
//! ## Config required
//! ```toml
//! # webhooks.toml
//! [webhooks."local".webhooks."counter"]
//! log_type = "counter"
//! headers = []
//! logbacks_allowed = { Limited = 0 }
//! ```
//!
//! ## Try it
//! ```sh
//! # Call multiple times to see counts increase
//! curl -s -X POST http://localhost:8080/webhook/counter -d 'increment'
//!
//! # Restart plaid, then call again — storage count persists, cache resets to 0
//! ```

use plaid_stl::{entrypoint_with_source_and_response, messages::LogSource, plaid};
use serde::Serialize;

entrypoint_with_source_and_response!();

const STORAGE_KEY: &str = "counter_value";
const CACHE_KEY: &str = "counter_value";

#[derive(Serialize)]
struct CounterState {
    storage_count: u64,
    cache_count: u64,
    note: &'static str,
}

fn main(_data: String, _source: LogSource) -> Result<Option<String>, i32> {
    // --- Persistent storage ---
    // Read current value from storage. If the key doesn't exist yet,
    // storage::get returns an error — we treat that as 0.
    let storage_count = match plaid::storage::get(STORAGE_KEY) {
        Ok(bytes) => {
            let s = String::from_utf8(bytes).unwrap_or_default();
            s.parse::<u64>().unwrap_or(0)
        }
        Err(_) => 0,
    };
    let new_storage_count = storage_count + 1;

    // Write the incremented value back. Storage takes &[u8].
    let value = new_storage_count.to_string();
    let _ = plaid::storage::insert(STORAGE_KEY, value.as_bytes());

    // --- In-memory cache ---
    // Read current value from cache. Same pattern: missing key = 0.
    let cache_count = match plaid::cache::get(CACHE_KEY) {
        Ok(s) => s.parse::<u64>().unwrap_or(0),
        Err(_) => 0,
    };
    let new_cache_count = cache_count + 1;

    // Write the incremented value back. Cache takes &str.
    let _ = plaid::cache::insert(CACHE_KEY, &new_cache_count.to_string());

    // Return both counts so the caller can see the difference.
    let state = CounterState {
        storage_count: new_storage_count,
        cache_count: new_cache_count,
        note: "Restart plaid to see cache reset while storage persists",
    };

    let body = serde_json::to_string_pretty(&state).unwrap();
    plaid::print_debug_string(&format!("[counter] storage={new_storage_count} cache={new_cache_count}"));
    Ok(Some(body))
}
