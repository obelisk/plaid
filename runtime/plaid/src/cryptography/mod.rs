pub mod aes_128_cbc;
pub mod hash;

#[derive(Debug)]
pub enum Errors {
    AesEncryptionFailure(String),
    AesDecryptionFailure(String),
}
