use super::Errors;
use aes::Aes128;
use base64::{decode_config, encode_config};
use block_modes::block_padding::Pkcs7;
use block_modes::{BlockMode, Cbc};
use rand::RngCore;

type Aes128Cbc = Cbc<Aes128, Pkcs7>;

/// Encrypts with AES-128-CBC + PKCS7. `key` must be 16 bytes.
/// Returns base64(iv||ciphertext).
pub fn encrypt(key: &[u8], plaintext: &str) -> Result<String, Errors> {
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);
    let cipher = Aes128Cbc::new_from_slices(key, &iv)
        .map_err(|_| Errors::AesEncryptionFailure("Failed to load key".to_string()))?;
    let ct = cipher.encrypt_vec(plaintext.as_bytes());
    let mut out = iv.to_vec();
    out.extend(ct);
    Ok(encode_config(out, base64::URL_SAFE))
}

/// Decrypts a base64-encoded blob (iv||ciphertext). `key` must be 16 bytes.
pub fn decrypt(key: &[u8], ciphertext: &str) -> Result<String, Errors> {
    let data = decode_config(ciphertext, base64::URL_SAFE)
        .map_err(|_| Errors::AesDecryptionFailure("Failed to decode ciphertext".to_string()))?;
    if data.len() < 16 {
        return Err(Errors::AesDecryptionFailure(
            "Input data too short".to_string(),
        ));
    }
    let (iv, ct) = data.split_at(16);
    let cipher = Aes128Cbc::new_from_slices(key, iv)
        .map_err(|_| Errors::AesDecryptionFailure("Failed to load key".to_string()))?;
    let pt = cipher
        .decrypt_vec(ct)
        .map_err(|_| Errors::AesDecryptionFailure("Failed to decrypt".to_string()))?;
    String::from_utf8(pt).map_err(|_| {
        Errors::AesDecryptionFailure("The output of the decryption is not a string".to_string())
    })
}
