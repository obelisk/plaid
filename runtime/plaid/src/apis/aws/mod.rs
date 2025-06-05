use kms::{Kms, KmsConfig};
use ecr::{Ecr, EcrConfig};
use serde::Deserialize;

pub mod ecr;
pub mod kms;

/// The entire configuration of AWS APIs implemented in Plaid
#[derive(Deserialize)]
pub struct AwsConfig {
    /// Configuration for the KMS API
    pub kms: KmsConfig,
    /// Configuration for the ECR API
    pub ecr: Option<EcrConfig>,
}

/// Contains all AWS services that Plaid implements APIs for
pub struct Aws {
    /// AWS Key Management Service
    pub kms: Kms,
    /// AWS Elastic Container Registry
    pub ecr: Option<Ecr>,
}

impl Aws {
    pub async fn new(config: AwsConfig) -> Self {
        let kms = Kms::new(config.kms).await;
        let ecr = match config.ecr {
            Some(ecr_config) => Some(Ecr::new(ecr_config).await),
            None => None,
        };

        Aws { kms, ecr }
    }

    /// List ECR repositories - wrapper to handle optional ECR
    pub async fn ecr_list_repositories(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, crate::apis::ApiError> {
        match &self.ecr {
            Some(ecr) => ecr.list_repositories(params, module).await,
            None => {
                error!("ECR API was called but not configured");
                Err(crate::apis::ApiError::ConfigurationError("ECR not configured".to_string()))
            }
        }
    }

    /// List ECR images - wrapper to handle optional ECR
    pub async fn ecr_list_images(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, crate::apis::ApiError> {
        match &self.ecr {
            Some(ecr) => ecr.list_images(params, module).await,
            None => {
                error!("ECR API was called but not configured");
                Err(crate::apis::ApiError::ConfigurationError("ECR not configured".to_string()))
            }
        }
    }

    /// Describe ECR images - wrapper to handle optional ECR
    pub async fn ecr_describe_images(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, crate::apis::ApiError> {
        match &self.ecr {
            Some(ecr) => ecr.describe_images(params, module).await,
            None => {
                error!("ECR API was called but not configured");
                Err(crate::apis::ApiError::ConfigurationError("ECR not configured".to_string()))
            }
        }
    }
}
