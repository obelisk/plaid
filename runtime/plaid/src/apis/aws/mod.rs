use dynamodb::{DynamoDb, DynamoDbConfig};
use kms::{Kms, KmsConfig};
use serde::Deserialize;

pub mod dynamodb;
pub mod kms;

/// The entire configuration of AWS APIs implemented in Plaid
#[derive(Deserialize)]
pub struct AwsConfig {
    /// Configuration for the KMS API
    pub kms: KmsConfig,
    pub dynamodb: Option<DynamoDbConfig>,
}

/// Contains all AWS services that Plaid implements APIs for
pub struct Aws {
    /// AWS Key Management Service
    pub kms: Kms,
    pub dynamodb: Option<DynamoDb>,
}

impl Aws {
    pub async fn new(config: AwsConfig) -> Self {
        let kms = Kms::new(config.kms).await;
        let dynamodb = if let Some(cfg) = config.dynamodb {
            Some(DynamoDb::new(cfg).await)
        } else {
            None
        };

        Aws { kms, dynamodb }
    }
}
