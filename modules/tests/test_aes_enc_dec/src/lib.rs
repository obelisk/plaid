use std::collections::HashMap;

use plaid_stl::{
    aes, entrypoint_with_source, messages::LogSource, network::make_named_request, plaid,
};

entrypoint_with_source!();

const PLAINTEXT: &str = "This is a test payload";

fn main(_: String, _: LogSource) -> Result<(), i32> {
    let ciphertext = aes::aes_encrypt_local_key("my_aes_key", PLAINTEXT).unwrap();
    plaid::print_debug_string(&format!("The ciphertext is {ciphertext}"));

    let decrypted = aes::aes_decrypt_local_key("my_aes_key", ciphertext).unwrap();
    if decrypted == PLAINTEXT {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Now try with a key that this rule is not permitted to use
    if aes::aes_encrypt_local_key("another_aes_key", PLAINTEXT).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Now try with a key that does not exist
    if aes::aes_encrypt_local_key("does_not_exist", PLAINTEXT).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }
    Ok(())
}
