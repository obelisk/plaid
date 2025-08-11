use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

/// Payload sent to the runtime when doing an AES encryption
#[derive(Serialize, Deserialize)]
pub struct AesEncryptPayload {
    pub key_id: String,
    pub plaintext: String,
}

/// Payload sent to the runtime when doing an AES decryption
#[derive(Serialize, Deserialize)]
pub struct AesDecryptPayload {
    pub key_id: String,
    pub ciphertext: String,
}

/// Encrypt a plaintext with a key defined in Plaid's config.
///
/// Args:
/// * `key_id` - The identifier of the key, as specified in Plaid's config
/// * `plaintext` - The string to be encrypted
pub fn aes_encrypt_local_key(
    key_id: &str,
    plaintext: impl Display,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aes, encrypt_local);
    }
    let payload = AesEncryptPayload {
        key_id: key_id.to_string(),
        plaintext: plaintext.to_string(),
    };

    let request = serde_json::to_string(&payload).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        aes_encrypt_local(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    Ok(String::from_utf8(return_buffer).unwrap())
}

/// Decrypt a ciphertext with a key defined in Plaid's config.
///
/// Args:
/// * `key_id` - The identifier of the key, as specified in Plaid's config
/// * `ciphertext` - The ciphertext to be decrypted
pub fn aes_decrypt_local_key(
    key_id: &str,
    ciphertext: impl Display,
) -> Result<String, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aes, decrypt_local);
    }
    let payload = AesDecryptPayload {
        key_id: key_id.to_string(),
        ciphertext: ciphertext.to_string(),
    };

    let request = serde_json::to_string(&payload).unwrap();

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        aes_decrypt_local(
            request.as_bytes().as_ptr(),
            request.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);
    // This should be safe because unless the Plaid runtime is expressly trying
    // to mess with us, this came from a String in the API module.
    Ok(String::from_utf8(return_buffer).unwrap())
}
