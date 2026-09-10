//! Shared test helpers for the Plaid runtime crate.

use std::sync::Arc;

use crate::loader::{CompilerBackend, LimitableAmount, LimitedAmount, LimitValue, PlaidModule};

/// Build a minimal `PlaidModule` for tests that need a module handle but do
/// not execute it. The wasm binary is a valid, empty module (just a type
/// section) so it compiles under any backend.
pub fn stub_module(name: &str, logtype: &str) -> Arc<PlaidModule> {
    // A minimal valid wasm binary: magic + version + a single empty type
    // section. Enough for `Module::new` to succeed.
    let wasm_bytes: Vec<u8> = vec![
        0x00, 0x61, 0x73, 0x6d, // \0asm
        0x01, 0x00, 0x00, 0x00, // version 1
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section: one () -> () func type
    ];

    let computation = LimitedAmount {
        default: 1_000_000,
        log_type: Default::default(),
        module_overrides: Default::default(),
    };
    let memory = LimitedAmount {
        default: 16,
        log_type: Default::default(),
        module_overrides: Default::default(),
    };
    let storage = LimitableAmount {
        default: LimitValue::Limited(1024),
        log_type: Default::default(),
        module_overrides: Default::default(),
    };

    #[cfg(feature = "cranelift")]
    let backend = CompilerBackend::Cranelift;
    #[cfg(all(not(feature = "cranelift"), feature = "llvm"))]
    let backend = CompilerBackend::LLVM;

    let module = PlaidModule::compile_for_tests(
        name,
        &computation,
        &memory,
        &storage,
        wasm_bytes,
        logtype,
        false,
        &backend,
    )
    .expect("stub module should compile");

    Arc::new(module)
}
