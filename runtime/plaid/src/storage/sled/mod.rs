//! This module provides a way for Plaid to use Sled as a DB for persistent storage.

use async_trait::async_trait;

use serde::Deserialize;

use sled::Db;

use super::{StorageError, StorageProvider};

/// Configuration for a Sled DB
#[derive(Deserialize)]
pub struct Config {
    sled_path: String,
}

/// A wrapper around a Sled DB object
pub struct Sled {
    db: Db,
}

impl Sled {
    pub fn new(config: Config) -> Result<Self, StorageError> {
        let db: sled::Db = sled::open(&config.sled_path)
            .map_err(|e| StorageError::CouldNotAccessStorage(e.to_string()))?;
        Ok(Self { db })
    }
}

#[async_trait]
impl StorageProvider for Sled {
    async fn insert(
        &self,
        namespace: String,
        key: String,
        value: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let tree = self
            .db
            .open_tree(namespace.as_bytes())
            .map_err(|_| StorageError::Access(format!("Could not open Sled tree {namespace}")))?;

        let result = tree.insert(key.as_bytes(), value).map_err(|_| {
            StorageError::Access(format!(
                "Could not access Sled value at {key} in {namespace}"
            ))
        })?;

        Ok(result.map(|v| v.to_vec()))
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let tree = self
            .db
            .open_tree(namespace.as_bytes())
            .map_err(|_| StorageError::Access(format!("Could not open Sled tree {namespace}")))?;

        let result = tree.get(key.as_bytes()).map_err(|_| {
            StorageError::Access(format!(
                "Could not access Sled value at {key} in {namespace}"
            ))
        })?;

        Ok(result.map(|v| v.to_vec()))
    }

    async fn delete(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let tree = self
            .db
            .open_tree(namespace.as_bytes())
            .map_err(|_| StorageError::Access(format!("Could not open Sled tree {namespace}")))?;

        let result = tree.remove(key.as_bytes()).map_err(|_| {
            StorageError::Access(format!(
                "Could not access Sled value at {key} in {namespace}"
            ))
        })?;

        Ok(result.map(|v| v.to_vec()))
    }

    async fn list_keys(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, StorageError> {
        let tree = self
            .db
            .open_tree(namespace.as_bytes())
            .map_err(|_| StorageError::Access(format!("Could not open Sled tree {namespace}")))?;

        let key_iter = match prefix {
            Some(p) => tree.scan_prefix(p),
            None => tree.iter(),
        };
        // The use of a filter_map here means keys that fail to be pulled will be thrown away.
        // I don't know if this is possible? Maybe if the database is moved out from under us?
        let keys: Vec<String> = key_iter
            .keys()
            .filter_map(|x| match x {
                Ok(v) => String::from_utf8(v.to_vec()).ok(),
                Err(e) => {
                    error!("Storage Error Listing Keys: {e}");
                    None
                }
            })
            .collect();

        Ok(keys)
    }

    async fn fetch_all(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>, StorageError> {
        let tree = self
            .db
            .open_tree(namespace.as_bytes())
            .map_err(|_| StorageError::Access(format!("Could not open Sled tree {namespace}")))?;

        let key_iter = match prefix {
            Some(p) => tree.scan_prefix(p),
            None => tree.iter(),
        };
        // The use of a filter_map here means keys that fail to be pulled will be thrown away.
        // I don't know if this is possible? Maybe if the database is moved out from under us?
        let data: Vec<(String, Vec<u8>)> = key_iter
            .filter_map(|x| match x {
                Ok((k, v)) => String::from_utf8(k.to_vec())
                    .ok()
                    .map(|key| (key, v.to_vec())),
                Err(e) => {
                    error!("Storage Error Listing Keys: {e}");
                    None
                }
            })
            .collect();

        Ok(data)
    }

    async fn get_namespace_byte_size(&self, namespace: &str) -> Result<u64, StorageError> {
        let all = self.fetch_all(namespace, None).await?;
        let mut counter = 0u64;
        for item in all {
            // Count bytes for keys and values
            counter += item.0.as_bytes().len() as u64 + item.1.len() as u64;
        }
        Ok(counter)
    }
}
