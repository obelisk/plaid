#[cfg(feature = "aws")]
use aws_config::{BehaviorVersion, Region, SdkConfig};
#[cfg(feature = "aws")]
use aws_sdk_kms::config::Credentials;

#[macro_use]
extern crate log;

pub mod apis;
pub mod config;
pub mod data;
pub mod executor;
pub mod functions;
pub mod loader;
pub mod logging;
pub mod performance;
pub mod storage;

/// Defines methods to authenticate to AWS with
#[cfg(feature = "aws")]
#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum AwsAuthentication {
    ApiKey {
        access_key_id: String,
        secret_access_key: String,
        session_token: Option<String>,
        region: String,
    },
    Iam {},
}

/// Get an `SdkConfig` to be used when interacting with AWS services
#[cfg(feature = "aws")]
pub async fn get_aws_sdk_config(authentication: AwsAuthentication) -> SdkConfig {
    match authentication {
        AwsAuthentication::ApiKey {
            access_key_id,
            secret_access_key,
            session_token,
            region,
        } => {
            info!("Using API keys for AWS authentication");
            let credentials = Credentials::new(
                access_key_id,
                secret_access_key,
                session_token,
                None,
                "Plaid",
            );

            aws_config::defaults(BehaviorVersion::latest())
                .region(Region::new(region.clone()))
                .credentials_provider(credentials)
                .load()
                .await
        }
        AwsAuthentication::Iam {} => {
            info!("Using IAM role assigned to environment for AWS authentication");
            aws_config::load_defaults(BehaviorVersion::latest()).await
        }
    }
}
