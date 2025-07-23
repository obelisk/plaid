//! This module provides a way for Plaid to use an in-memory cache.

use crate::cache::CacheError;
use async_trait::async_trait;
use lru::LruCache;
use std::{collections::HashMap, num::NonZeroUsize};
use tokio::sync::RwLock;

/// A wrapper for the cache
pub struct InMemoryCache {
    /// This is mapping module names to LruCache objects: one cache per module
    caches: HashMap<String, RwLock<LruCache<String, String>>>,
}

impl InMemoryCache {
    pub fn new(modules_and_logtypes: HashMap<String, String>, config: super::Config) -> Self {
        // Create an LruCache for each module and collect everything into a HashMap
        let caches: HashMap<String, RwLock<LruCache<String, String>>> = modules_and_logtypes
            .iter()
            .map(|(module, logtype)| {
                // Figure out the capacity of the cache for this module
                let mut capacity = config.cache_entries.default;
                if let Some(v) = config.cache_entries.log_type.get(logtype) {
                    capacity = *v;
                }
                if let Some(v) = config.cache_entries.module_overrides.get(module) {
                    capacity = *v;
                }

                let module_cache =
                    RwLock::new(LruCache::new(NonZeroUsize::new(capacity as usize).unwrap()));

                (module.to_string(), module_cache)
            })
            .collect();

        InMemoryCache { caches }
    }
}

#[async_trait]
impl super::CacheProvider for InMemoryCache {
    async fn put(
        &self,
        namespace: &str,
        key: &str,
        value: &str,
    ) -> Result<Option<String>, CacheError> {
        let module_cache = self
            .caches
            .get(namespace)
            .ok_or(CacheError::CacheAccessError(format!(
                "Cache not found for module {namespace}"
            )))?;
        Ok(module_cache
            .write()
            .await
            .put(key.to_string(), value.to_string()))
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, CacheError> {
        let module_cache = self
            .caches
            .get(namespace)
            .ok_or(CacheError::CacheAccessError(format!(
                "Cache not found for module {namespace}"
            )))?;
        Ok(module_cache.write().await.get(&key.to_string()).cloned())
    }
}
