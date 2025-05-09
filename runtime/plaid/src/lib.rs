use std::collections::HashMap;

#[cfg(feature = "aws")]
use aws_config::{BehaviorVersion, Region, SdkConfig};
#[cfg(feature = "aws")]
use aws_sdk_kms::config::Credentials;
use crossbeam_channel::{Receiver, Sender};
use executor::Message;

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

/// A pool of threads to process logs
#[derive(Clone)]
pub struct ThreadPool {
    pub num_threads: u8,
    pub sender: Sender<Message>,
    pub receiver: Receiver<Message>,
}

/// A struct that keeps track of all Plaid's thread pools
#[derive(Clone)]
pub struct ExecutionThreadPools {
    /// Thread pool for general processing, i.e., for processing logs
    /// which do not have a dedicated thread pool.
    pub general_pool: ThreadPool,
    /// Thread pools dedicated to specific log types.
    /// Mapping { log_type --> thread_pool }
    pub dedicated_pools: HashMap<String, ThreadPool>,
}

impl ExecutionThreadPools {
    /// Create a new ExecutionThreadPools object by initializing only the thread
    /// pool for general processing. Other thread pools, if present, must be
    /// added separately by inserting into the `dedicated_pools` map.
    pub fn new(num_threads: u8, sender: Sender<Message>, receiver: Receiver<Message>) -> Self {
        ExecutionThreadPools {
            general_pool: ThreadPool {
                num_threads,
                sender,
                receiver,
            },
            dedicated_pools: HashMap::new(),
        }
    }
}

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
