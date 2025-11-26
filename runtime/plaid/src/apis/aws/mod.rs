use dynamodb::{DynamoDb, DynamoDbConfig};
use kms::{Kms, KmsConfig};
use s3::{S3Config, S3};
use serde::Deserialize;

pub mod dynamodb;
pub mod dynamodb_utils;
pub mod kms;
pub mod s3;

/// The entire configuration of AWS APIs implemented in Plaid
#[derive(Deserialize)]
pub struct AwsConfig {
    /// Configuration for the KMS API
    pub kms: Option<KmsConfig>,
    /// Configuration for the S3 API
    pub s3: Option<S3Config>,
    /// Configuration for the DynamoDB API
    pub dynamodb: Option<DynamoDbConfig>,
}

/// Contains all AWS services that Plaid implements APIs for
pub struct Aws {
    /// AWS Key Management Service
    pub kms: Option<Kms>,
    /// AWS Simple Storage Service
    pub s3: Option<S3>,
    /// AWS DynamoDB Service
    pub dynamodb: Option<DynamoDb>,
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
        let dynamodb = match config.dynamodb {
            Some(conf) => Some(DynamoDb::new(conf).await),
            None => None,
        };

        Aws { kms, s3, dynamodb }
    }
}
