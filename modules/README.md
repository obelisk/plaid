# Modules

This workspace is used to compile modules and is separate from the runtime workspace because the target here is always `wasm32-unknown-unknown`. We have to do this until `per-package-target` is stabilized so for now this will do.

Modules here are organized into different categories to showcase the different things Plaid can use them for. The `tests` directory is just for modules which are used in the integration test runs and have bash harnesses to that effect that other modules will not.