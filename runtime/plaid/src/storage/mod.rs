use async_trait::async_trait;

mod sled;

use serde::Deserialize;

/// Plaid's storage configuration
#[derive(Deserialize)]
pub struct Config {
    pub sled: Option<sled::Config>,
}

/// The storage that underpins Plaid
pub struct Storage {
    database: Box<dyn StorageProvider + Send + Sync>,
}

/// Errors encountered while trying to use Plaid's persistent storage.
#[derive(Debug)]
pub enum StorageError {
    NoStorageConfigured,
    CouldNotAccessStorage(String),
    Access(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoStorageConfigured => {
                write!(f, "No storage system configuration could be found")
            }
            Self::CouldNotAccessStorage(ref e) => {
                write!(f, "Access the storage datastore was not possible: {e}")
            }
            StorageError::Access(ref e) => write!(f, "There was a failure accessing a key: {e}"),
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
    async fn list_keys(&self, namespace: &str, prefix: Option<&str>) -> Result<Vec<String>, StorageError>;
    /// Same as list_keys but will return the keys and values. An optional prefix can be provided
    /// but this only applies to the key, values have no host provided filtering.
    async fn fetch_all(&self, namespace: &str, prefix: Option<&str>) -> Result<Vec<(String, Vec<u8>)>, StorageError>;
}

impl Storage {
    pub fn new(config: Config) -> Result<Self, StorageError> {
        let database = match config.sled {
            Some(sled) => Box::new(sled::Sled::new(sled)?),
            _ => return Err(StorageError::NoStorageConfigured),
        };

        Ok(Storage { database })
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

    pub async fn list_keys(&self, namespace: &str, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        self.database.list_keys(namespace, prefix).await
    }

    pub async fn fetch_all(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>, StorageError> {
        self.database.fetch_all(namespace, prefix).await
    }
}
