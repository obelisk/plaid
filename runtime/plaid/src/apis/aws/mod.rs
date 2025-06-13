use kms::{Kms, KmsConfig};
use serde::Deserialize;

pub mod iam;
pub mod kms;

/// The entire configuration of AWS APIs implemented in Plaid
#[derive(Deserialize)]
pub struct AwsConfig {
    /// Configuration for the KMS API
    pub kms: KmsConfig,
}

/// Contains all AWS services that Plaid implements APIs for
pub struct Aws {
    /// AWS Key Management Service
    pub kms: Kms,
}

impl Aws {
    pub async fn new(config: AwsConfig) -> Self {
        let kms = Kms::new(config.kms).await;

        Aws { kms }
    }
}
