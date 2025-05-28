use kms::{Kms, KmsConfig};
use s3::{S3Config, S3};
use serde::Deserialize;

pub mod kms;
pub mod s3;

/// The entire configuration of AWS APIs implemented in Plaid
#[derive(Deserialize)]
pub struct AwsConfig {
    /// Configuration for the KMS API
    pub kms: Option<KmsConfig>,
    pub s3: Option<S3Config>,
}

/// Contains all AWS services that Plaid implements APIs for
pub struct Aws {
    /// AWS Key Management Service
    pub kms: Option<Kms>,
    /// AWS Simple Storage Service
    pub s3: Option<S3>,
}

impl Aws {
    pub async fn new(config: AwsConfig) -> Self {
        let kms = match config.kms {
            Some(conf) => Some(Kms::new(conf).await),
            None => None,
        };
        let s3 = match config.s3 {
            Some(conf) => Some(S3::new(conf).await),
            None => None,
        };

        Aws { kms, s3 }
    }
}
