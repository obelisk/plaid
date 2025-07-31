//! This module provides a way for Plaid to use Redis as a cache layer

use async_trait::async_trait;
use serde::Deserialize;

use crate::cache::CacheError;

use redis::{aio::MultiplexedConnection, AsyncCommands};

/// A wrapper for the cache
pub struct RedisCache {
    connection: MultiplexedConnection,
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub url: String,
}

impl RedisCache {
    pub async fn new(config: Config) -> Result<Self, CacheError> {
        let url = config.url.strip_prefix("redis://").unwrap_or(&config.url);
        let client = redis::Client::open(format!("redis://{url}")).map_err(|e| {
            CacheError::CacheInitError(format!("Could not create redis client: {e}"))
        })?;
        let connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| {
                CacheError::CacheInitError(format!("Could not create redis connection: {e}"))
            })?;
        Ok(Self { connection })
    }
}

#[async_trait]
impl super::CacheProvider for RedisCache {
    async fn put(&mut self, key: &str, value: &str) -> Result<Option<String>, CacheError> {
        let old = self
            .connection
            .get(key)
            .await
            .map_err(|e| CacheError::GetError(e.to_string()))?;
        let () = self
            .connection
            .set(key, value)
            .await
            .map_err(|e| CacheError::PutError(e.to_string()))?;
        Ok(old)
    }

    async fn get(&mut self, key: &str) -> Result<Option<String>, CacheError> {
        let r = self
            .connection
            .get(key)
            .await
            .map_err(|e| CacheError::GetError(e.to_string()))?;
        Ok(r)
    }
}
