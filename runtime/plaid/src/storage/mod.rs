use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;

#[cfg(feature = "aws")]
pub mod dynamodb;

#[cfg(feature = "sled")]
pub mod sled;

use futures_util::future::join_all;
use serde::Deserialize;

use crate::loader::LimitValue;

/// Config of a shared DB
#[derive(Deserialize)]
pub struct SharedDbConfig {
    /// The max size of this shared DB
    pub size_limit: LimitValue,
    /// List of rules that can read from the DB
    #[serde(default)]
    pub r: Vec<String>,
    /// List of rules that can read from and write to the DB
    #[serde(default)]
    pub rw: Vec<String>,
}

/// Represents a shared DB in the system
pub struct SharedDb {
    /// Configuration for the shared DB
    pub config: SharedDbConfig,
    /// Counter for the storage used by the shared DB
    pub used_storage: Arc<RwLock<u64>>,
}

/// Plaid's DB layer
#[derive(Deserialize)]
#[serde(untagged)]
pub enum DatabaseConfig {
    #[cfg(feature = "sled")]
    Sled(sled::Config),
    #[cfg(feature = "aws")]
    DynamoDb(dynamodb::Config),
}

/// Plaid's storage configuration
#[derive(Deserialize)]
pub struct Config {
    pub db: Option<DatabaseConfig>,
    /// Map `{ db_name --> db_config }`  
    /// Note - `db_name` must not terminate with ".wasm" to avoid confusing it with a rule-specific namespace
    pub shared_dbs: Option<HashMap<String, SharedDbConfig>>,
}

/// The storage that underpins Plaid
pub struct Storage {
    database: Box<dyn StorageProvider + Send + Sync>,
    pub shared_dbs: Option<HashMap<String, SharedDb>>,
}

/// Errors encountered while trying to use Plaid's persistent storage.
#[derive(Debug)]
pub enum StorageError {
    StorageInitError(String),
    NoStorageConfigured,
    CouldNotAccessStorage(String),
    Access(String),
    SharedDbError(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StorageInitError(ref e) => write!(f, "Error while initializing storage: {e}"),
            Self::NoStorageConfigured => {
                write!(f, "No storage system configuration could be found")
            }
            Self::CouldNotAccessStorage(ref e) => {
                write!(f, "Access the storage datastore was not possible: {e}")
            }
            Self::Access(ref e) => write!(f, "There was a failure accessing a key: {e}"),
            Self::SharedDbError(ref e) => {
                write!(f, "Error while attempting an operation on a shared DB: {e}")
            }
        }
    }
}

impl std::error::Error for StorageError {}

/// Defines the basic methods that all storage providers must offer.
#[async_trait]
pub trait StorageProvider {
    /// Insert a new key pair into the storage provider
    async fn insert(
        &self,
        namespace: String,
        key: String,
        value: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, StorageError>;
    /// Get a value by key from the storage provider. If the key doesn't exist, then it will
    /// return Ok(None) signifying the storage provider was successfully able to identify
    /// the key was not set.
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    /// Delete a value by key from the storage provider. If the key exists this will return
    /// Ok(Some(previous_value)), if not, Ok(None)
    async fn delete(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    /// List all keys in the given namespace. An optional prefix can be provided such that only
    /// specific keys can be returned. This is helpful as it reduces the amount of compute that
    /// needs to be taken by modules to do basic filtering. More complex filtering (i.e regex)
    /// is not supported as computation for that has unbounded complexity.
    async fn list_keys(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, StorageError>;
    /// Same as list_keys but will return the keys and values. An optional prefix can be provided
    /// but this only applies to the key, values have no host provided filtering.
    async fn fetch_all(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>, StorageError>;
    /// Get the number of bytes stored in a namespace. This will include keys and values.
    async fn get_namespace_byte_size(&self, namespace: &str) -> Result<u64, StorageError>;
    /// Apply a migration to all the entries of a namespace by computing a function on each `(key,value)`
    /// pair. The old pair is deleted from the DB and the new one is inserted.  
    /// Note - The function is applied in a random order over the entries. If the function is not injective over the space
    /// of DB keys (i.e., it produces two equal keys for different inputs), then one of the entries will be overwritten.
    async fn apply_migration(
        &self,
        namespace: &str,
        f: Box<dyn Fn(String, Vec<u8>) -> (String, Vec<u8>) + Send + Sync>,
    ) -> Result<(), StorageError>;
}

impl Storage {
    pub async fn new(config: Config) -> Result<Self, StorageError> {
        // Try building a database from the values in the config
        let database: Box<dyn StorageProvider + Send + Sync> = match config.db {
            #[cfg(feature = "sled")]
            Some(DatabaseConfig::Sled(sled)) => Box::new(sled::Sled::new(sled)?),
            #[cfg(feature = "aws")]
            Some(DatabaseConfig::DynamoDb(dynamodb)) => Box::new(
                dynamodb::DynamoDb::new(dynamodb)
                    .await
                    .map_err(|e| StorageError::StorageInitError(e))?,
            ),
            _ => {
                return Err(StorageError::NoStorageConfigured);
            }
        };

        let shared_dbs = config
            .shared_dbs
            .map(async |shared_dbs| {
                join_all(shared_dbs.into_iter().map(async |(db_name, db_config)| {
                    if db_name.to_string().ends_with(".wasm") {
                        return Err(StorageError::SharedDbError(
                            "The name of a shared DB must not end with .wasm".to_string(),
                        ));
                    }
                    let used_storage = match database.get_namespace_byte_size(&db_name).await {
                        Ok(r) => r,
                        Err(_) => {
                            return Err(StorageError::SharedDbError(
                                "Could not count used storage in shared DB".to_string(),
                            ))
                        }
                    };
                    let db = SharedDb {
                        config: db_config,
                        used_storage: Arc::new(RwLock::new(used_storage)),
                    };
                    Ok((db_name, db))
                }))
                .await
                .into_iter()
                .collect::<Result<HashMap<_, _>, _>>()
            })
            .ok_or(StorageError::SharedDbError(
                "Error while configuring shared storage".to_string(),
            ))?
            .await
            .ok();

        Ok(Storage {
            database,
            shared_dbs,
        })
    }

    pub async fn insert(
        &self,
        namespace: String,
        key: String,
        value: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.database.insert(namespace, key, value).await
    }

    pub async fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.database.get(namespace, key).await
    }

    pub async fn delete(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.database.delete(namespace, key).await
    }

    pub async fn list_keys(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, StorageError> {
        self.database.list_keys(namespace, prefix).await
    }

    pub async fn fetch_all(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>, StorageError> {
        self.database.fetch_all(namespace, prefix).await
    }

    pub async fn get_namespace_byte_size(&self, namespace: &str) -> Result<u64, StorageError> {
        self.database.get_namespace_byte_size(namespace).await
    }
}
