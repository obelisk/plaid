//! This module provides a way for Plaid to use an in-memory store as a DB. Note - This storage is not persisted across reboots.

use super::{StorageError, StorageProvider};
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

pub struct InMemoryDb {
    db: Arc<RwLock<HashMap<String, HashMap<String, Vec<u8>>>>>,
}

impl InMemoryDb {
    pub fn new() -> Result<Self, StorageError> {
        Ok(Self {
            db: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}

#[async_trait]
impl StorageProvider for InMemoryDb {
    async fn insert(
        &self,
        namespace: String,
        key: String,
        value: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let mut db = self.db.write().await;
        let ns = db.entry(namespace).or_default();
        Ok(ns.insert(key, value))
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let db = self.db.read().await;
        Ok(db.get(namespace).and_then(|ns| ns.get(key).cloned()))
    }

    async fn delete(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let mut db = self.db.write().await;
        if let Some(ns) = db.get_mut(namespace) {
            Ok(ns.remove(key))
        } else {
            Ok(None)
        }
    }

    async fn list_keys(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, StorageError> {
        let keys = self
            .db
            .read()
            .await
            .get(namespace)
            .map(|ns| {
                ns.keys()
                    .filter(|k| prefix.map_or(true, |p| k.starts_with(p)))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Ok(keys)
    }

    async fn fetch_all(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>, StorageError> {
        let db = self.db.read().await;
        let values = db
            .get(namespace)
            .map(|ns| {
                ns.iter()
                    .filter(|(k, _)| prefix.map_or(true, |p| k.starts_with(p)))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(values)
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

    async fn apply_migration(
        &self,
        namespace: &str,
        f: Box<dyn Fn(String, Vec<u8>) -> (String, Vec<u8>) + Send + Sync>,
    ) -> Result<(), StorageError> {
        // Get all the data for this namespace
        let data = self.fetch_all(namespace, None).await?;
        // For each key/value pair, perform the migration...
        for (key, value) in data {
            // Apply the transformation and obtain a new key and new value
            let (new_key, new_value) = f(key.clone(), value);
            // Delete the old entry because we are about to insert the new one
            self.delete(namespace, &key).await?;
            // And insert the new pair
            self.insert(namespace.to_string(), new_key, new_value)
                .await?;
        }
        Ok(())
    }
}
