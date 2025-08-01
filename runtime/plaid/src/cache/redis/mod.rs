//! This module provides a way for Plaid to use Redis as a cache layer

use async_trait::async_trait;
use serde::Deserialize;

use crate::cache::CacheError;

use redis::{AsyncCommands, Client};

/// A wrapper for the cache
pub struct RedisCache {
    // connection: MultiplexedConnection,
    client: Client,
}

/// Configuration for the Redis cache
#[derive(Deserialize, Clone)]
pub struct Config {
    pub hostname: String,
    pub port: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl std::fmt::Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let output = format!(
            "Hostname: {} | Port: {} | Username: {} | Password: {}",
            self.hostname,
            self.port.clone().unwrap_or("NA".to_string()),
            self.username.clone().unwrap_or("NA".to_string()),
            if self.password.is_some() {
                "********".to_string()
            } else {
                "[password not set]".to_string()
            }
        );
        write!(f, "{output}")
    }
}

impl Config {
    /// Build a full Redis connection string from its constituents
    pub fn build_connection_string(&self) -> String {
        let hostname = self
            .hostname
            .strip_prefix("redis://")
            .unwrap_or(&self.hostname);
        let port = match self.port {
            Some(ref p) => format!(":{p}"),
            None => String::new(),
        };
        let username = self.username.clone().unwrap_or(String::new());
        let password = match self.password {
            Some(ref p) => format!(":{p}"),
            None => String::new(),
        };
        let at = if username.is_empty() && password.is_empty() {
            String::new()
        } else {
            "@".to_string()
        };

        format!("redis://{username}{password}{at}{hostname}{port}")
    }
}

impl RedisCache {
    pub async fn new(config: Config) -> Result<Self, CacheError> {
        let conn_string = config.build_connection_string();
        let client = redis::Client::open(conn_string).map_err(|e| {
            CacheError::CacheInitError(format!("Could not create redis client: {e}"))
        })?;

        Ok(Self { client })
    }
}

#[async_trait]
impl super::CacheProvider for RedisCache {
    async fn put(
        &self,
        namespace: &str,
        key: &str,
        value: &str,
    ) -> Result<Option<String>, CacheError> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| {
                CacheError::CacheAccessError(format!("Could not create redis connection: {e}"))
            })?;
        let old = connection
            .hget(namespace, key)
            .await
            .map_err(|e| CacheError::GetError(e.to_string()))?;
        let () = connection
            .hset(namespace, key, value)
            .await
            .map_err(|e| CacheError::PutError(e.to_string()))?;
        Ok(old)
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, CacheError> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| {
                CacheError::CacheAccessError(format!("Could not create redis connection: {e}"))
            })?;
        let r = connection
            .hget(namespace, key)
            .await
            .map_err(|e| CacheError::GetError(e.to_string()))?;
        Ok(r)
    }
}
