use kms::{Kms, KmsConfig};
use serde::Deserialize;

pub mod kms;

/// The entire configuration of AWS APIs implemented in Plaid
#[derive(Deserialize)]
pub struct AwsConfig {
    pub kms: KmsConfig,
}

/// Contains all AWS services that Plaid implements APIs for
pub struct Aws {
    pub kms: Kms,
}

impl Aws {
    pub async fn new(config: AwsConfig) -> Self {
        let kms = Kms::new(config.kms).await;

        Aws { kms }
    }
}
