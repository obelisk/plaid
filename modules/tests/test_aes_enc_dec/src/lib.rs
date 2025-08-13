use std::collections::HashMap;

use plaid_stl::{
    cryptography, entrypoint_with_source, messages::LogSource, network::make_named_request, plaid,
};

entrypoint_with_source!();

const PLAINTEXT: &str = "This is a test payload";

fn main(_: String, _: LogSource) -> Result<(), i32> {
    let ciphertext = cryptography::aes_128_cbc_encrypt("my_aes_key", PLAINTEXT).unwrap();
    plaid::print_debug_string(&format!("The ciphertext is {ciphertext}"));

    let decrypted = cryptography::aes_128_cbc_decrypt("my_aes_key", ciphertext).unwrap();
    if decrypted == PLAINTEXT {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Now try with a key that this rule is not permitted to use
    if cryptography::aes_128_cbc_encrypt("another_aes_key", PLAINTEXT).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Now try with a key that does not exist
    if cryptography::aes_128_cbc_encrypt("does_not_exist", PLAINTEXT).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }

    // Now try with a key that this rule can only encrypt with. Encryption should work, decryption should fail
    let ciphertext = cryptography::aes_128_cbc_encrypt("aes_key_only_enc", PLAINTEXT).unwrap();
    if cryptography::aes_128_cbc_decrypt("aes_key_only_enc", ciphertext).is_err() {
        make_named_request("test-response", "OK", HashMap::new()).unwrap();
    }
    Ok(())
}
