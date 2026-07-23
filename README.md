# Plaid
A relatively simple system for automations and log processing.

## Building And Testing
Quick way to build and run a test module:
```
./testing/just_run_please.sh
```

To build the runtime:
```
cd runtime
cargo build
```

To run integration tests:
```
./testing/integration.sh [compiler]
```

For the feature-gated read-only PostgreSQL API, see the
[production configuration and database-role guide](POSTGRES.md).

## Releasing A New Version
The version must be bumped in two places:
* `runtime/plaid/Cargo.toml`
* `runtime/plaid-stl/Cargo.toml`

These versions are supposed to stay in sync all the time.

Once that's done and merged, a new release can be cut directly from the GH web UI.
