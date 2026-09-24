//! This module provides a way for Plaid to use Sled as a DB for persistent storage.

use async_trait::async_trait;

use serde::Deserialize;

use sled::{
    transaction::{ConflictableTransactionError, TransactionError},
    Db,
};

use super::{Item, StorageError, StorageProvider};

/// Configuration for a Sled DB
#[derive(Deserialize)]
pub struct Config {
    pub sled_path: String,
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
    fn is_persistent(&self) -> bool {
        true
    }

    async fn insert_batch(&self, namespace: String, items: Vec<Item>) -> Result<(), StorageError> {
        let tree = self
            .db
            .open_tree(namespace.as_bytes())
            .map_err(|_| StorageError::Access(format!("Could not open Sled tree {namespace}")))?;

        // Sled transactions are atomic and serializable: either every insert in the closure
        // is applied or none of them are. If a concurrent transaction conflicts with this
        // one, sled automatically retries the closure, so it must only borrow its captures.
        tree.transaction(|tx: &sled::transaction::TransactionalTree| {
            for item in items.iter() {
                tx.insert(item.key.as_bytes(), item.value.as_slice())?;
            }
            Ok::<(), ConflictableTransactionError<()>>(())
        })
        .map_err(|e| match e {
            TransactionError::Abort(_) => {
                StorageError::BatchWriteError("Transaction aborted".to_string())
            }
            TransactionError::Storage(e) => StorageError::BatchWriteError(e.to_string()),
        })?;

        Ok(())
    }

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
    ) -> Result<Vec<(String, Option<Vec<u8>>)>, StorageError> {
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
        let data = key_iter
            .filter_map(|x| match x {
                Ok((k, v)) => String::from_utf8(k.to_vec())
                    .ok()
                    .map(|key| (key, Some(v.to_vec()))),
                Err(e) => {
                    error!("Storage Error Listing Keys: {e}");
                    None
                }
            })
            .collect();

        Ok(data)
    }
}
