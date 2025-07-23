//! This module provides a way for Plaid to use an in-memory cache.

use std::num::NonZeroUsize;

use async_trait::async_trait;

use lru::LruCache;

use crate::cache::CacheError;

/// A wrapper for the cache
pub struct InMemoryCache {
    cache: LruCache<String, String>,
}

impl InMemoryCache {
    pub fn new(capacity: usize) -> Self {
        let cache = LruCache::new(NonZeroUsize::new(capacity).unwrap());
        InMemoryCache { cache }
    }
}

#[async_trait]
impl super::CacheProvider for InMemoryCache {
    async fn put(&mut self, key: &str, value: &str) -> Result<Option<String>, CacheError> {
        Ok(self.cache.put(key.to_string(), value.to_string()))
    }

    async fn get(&mut self, key: &str) -> Result<Option<String>, CacheError> {
        Ok(self.cache.get(&key.to_string()).cloned())
    }
}
