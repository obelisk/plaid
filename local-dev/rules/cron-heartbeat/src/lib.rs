//! # Cron Heartbeat Example
//!
//! Demonstrates how to create a rule that runs on a timer (cron schedule)
//! rather than being triggered by a webhook.
//!
//! The rule fires every 30 seconds, logs the current time, and tracks
//! invocation count in storage.
//!
//! ## Key concepts
//! - Cron rules are configured in `data.toml`, not `webhooks.toml`
//! - The `source` will be `LogSource::Generator(Generator::Interval("heartbeat"))`
//! - The `data` payload for cron-triggered rules is empty
//! - You can use `plaid::get_time()` to get the current unix timestamp
//!
//! ## Config required
//! ```toml
//! # data.toml
//! [data.interval]
//! [data.interval.jobs."heartbeat"]
//! schedule = "0,30 * * * * * *"
//! log_type = "cron_heartbeat"
//! ```
//!
//! ## Try it
//! ```sh
//! # No curl needed — watch the docker logs:
//! docker compose logs -f plaid 2>&1 | grep heartbeat
//! ```

use plaid_stl::{entrypoint_with_source, messages::LogSource, plaid};

entrypoint_with_source!();

const INVOCATION_KEY: &str = "heartbeat_count";

fn main(_data: String, source: LogSource) -> Result<(), i32> {
    let now = plaid::get_time();

    // Track how many times we've fired.
    let count = match plaid::storage::get(INVOCATION_KEY) {
        Ok(bytes) => {
            let s = String::from_utf8(bytes).unwrap_or_default();
            s.parse::<u64>().unwrap_or(0)
        }
        Err(_) => 0,
    };
    let new_count = count + 1;
    let _ = plaid::storage::insert(INVOCATION_KEY, new_count.to_string().as_bytes());

    plaid::print_debug_string(&format!(
        "[cron-heartbeat] tick #{new_count} at unix={now} source={source}"
    ));

    Ok(())
}
