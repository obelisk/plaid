#[cfg(feature = "aws")]
use aws_config::{BehaviorVersion, Region, SdkConfig};
#[cfg(feature = "aws")]
use aws_sdk_kms::config::Credentials;

#[macro_use]
extern crate log;

pub mod apis;
pub mod cache;
pub mod config;
pub mod cryptography;
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
pub async fn get_aws_sdk_config(authentication: &AwsAuthentication) -> SdkConfig {
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
                session_token.clone(),
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

/// The roles that this instance has, i.e., what this instance is running
#[derive(Debug, Clone, serde::Deserialize)]
pub struct InstanceRoles {
    /// Whether this instance is running webhooks
    pub webhooks: bool,
    /// Whether this instance is running data generators
    pub data_generators: bool,
    /// Whether this instance is running interval jobs defined in the config
    pub interval_jobs: bool,
    /// Whether this instance is running logbacks stored in Plaid's persistent storage.
    /// Note - All instances can send logbacks. This setting is just controlling which
    /// instance is _executing_ the logbacks that were queued.
    pub logbacks: bool,
    /// Whether this instance is running special log types that are marked as non-concurrent.
    /// It is the responsibility of the admin to make sure that, in a multi-instance deployment,
    /// this is set to true on exactly one instance.
    pub non_concurrent_rules: bool,
}

impl Default for InstanceRoles {
    /// By default, run everything
    fn default() -> Self {
        Self {
            webhooks: true,
            data_generators: true,
            interval_jobs: true,
            logbacks: true,
            non_concurrent_rules: true,
        }
    }
}
