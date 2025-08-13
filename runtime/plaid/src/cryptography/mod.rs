pub mod aes_128_cbc;

#[derive(Debug)]
pub enum Errors {
    AesEncryptionFailure(String),
    AesDecryptionFailure(String),
}
