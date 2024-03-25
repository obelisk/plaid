use async_trait::async_trait;

mod sled;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub sled: Option<sled::Config>,
}

pub struct Storage {
    database: Box<dyn StorageProvider + Send + Sync>,
}

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

#[async_trait]
pub trait StorageProvider {
    async fn insert(
        &self,
        namespace: String,
        key: String,
        value: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, StorageError>;
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    async fn delete(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    async fn list_keys(&self, namespace: &str) -> Result<Vec<Vec<u8>>, StorageError>;
    async fn fetch_all(&self, namespace: &str) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StorageError>;
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

    pub async fn list_keys(&self, namespace: &str) -> Result<Vec<Vec<u8>>, StorageError> {
        self.database.list_keys(namespace).await
    }

    pub async fn fetch_all(
        &self,
        namespace: &str,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StorageError> {
        self.database.fetch_all(namespace).await
    }
}
