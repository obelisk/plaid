use serde::Deserialize;

use crate::apis::cryptography::aes::{Aes, AesConfig};

pub mod aes;

#[derive(Deserialize)]
pub struct CryptographyConfig {
    aes: Option<AesConfig>,
}

pub struct Cryptography {
    aes: Option<Aes>,
}

impl Cryptography {
    pub fn new(config: CryptographyConfig) -> Self {
        let aes = config.aes.and_then(|aes_config| Some(Aes::new(aes_config)));

        Self { aes }
    }
}
