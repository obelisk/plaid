use std::env;
use std::fs;
use std::process;
use std::sync::Arc;

use wasmer::sys::{CompilerConfig, Cranelift};
use wasmer::{Engine, Module};
use wasmer_middlewares::metering::Metering;

use plaid::loader::cost_function;

/// Compile a single WASM file to verify it would load properly.
/// This function uses Cranelift compiler backend with default settings.
///
/// Returns Ok(()) if the module compiles successfully, Err otherwise.
async fn verify_module_compiles(wasm_path: &str) -> Result<(), String> {
    // Read the WASM file
    let module_bytes = fs::read(wasm_path).map_err(|e| format!("Failed to read WASM file: {e}"))?;

    // Set up default computation limit for verification
    let computation_limit = 10_000_000_u64;
    let metering = Arc::new(Metering::new(computation_limit, cost_function));

    // Configure Cranelift compiler with metering middleware
    let mut compiler = Cranelift::default();
    compiler.push_middleware(metering);
    let engine: Engine = compiler.into();

    // Attempt to compile the module
    Module::new(&engine, module_bytes).map_err(|e| format!("Failed to compile module: {e}"))?;

    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <path-to-wasm-file>", args[0]);
        process::exit(1);
    }

    let wasm_path = &args[1];

    match verify_module_compiles(wasm_path).await {
        Ok(()) => {
            println!("✅ Module compiles successfully: {}", wasm_path);
            process::exit(0);
        }
        Err(e) => {
            eprintln!("❌ Module compilation failed: {}", e);
            process::exit(1);
        }
    }
}
