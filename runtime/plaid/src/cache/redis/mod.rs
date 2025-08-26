//! This module provides a way for Plaid to use Redis as a cache layer

use crate::cache::CacheError;
use async_trait::async_trait;
use redis::{aio::ConnectionManager, AsyncCommands};
use ring::rand::SecureRandom;
use serde::Deserialize;

/// How entries are evicted from the Redis cache
#[derive(Deserialize, Clone)]
pub enum EvictionPolicy {
    /// Entries are never evicted from Redis
    NoEviction,
    /// When the max number of entries is reached, an entry is
    /// evicted at random to make space for new insertions.
    RandomEviction(usize),
}

impl std::fmt::Display for EvictionPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let output = match self {
            Self::NoEviction => "no eviction".to_string(),
            Self::RandomEviction(max) => format!("random eviction: max {max} entries"),
        };
        write!(f, "{output}")
    }
}

impl Default for EvictionPolicy {
    fn default() -> Self {
        Self::NoEviction
    }
}

/// A wrapper for the cache
pub struct RedisCache {
    connection_manager: ConnectionManager,
    eviction_policy: EvictionPolicy,
}

/// Configuration for the Redis cache
#[derive(Deserialize, Clone)]
pub struct Config {
    pub hostname: String,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub eviction_policy: Option<EvictionPolicy>,
}

impl std::fmt::Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let output = format!(
            "Hostname: {} | Port: {} | Username: {} | Password: {} | Eviction policy: {}",
            self.hostname,
            self.port.clone().unwrap_or(6379),
            self.username.clone().unwrap_or("NA".to_string()),
            if self.password.is_some() {
                "********".to_string()
            } else {
                "[password not set]".to_string()
            },
            self.eviction_policy.clone().unwrap_or_default()
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
        let connection_manager = client
            .get_connection_manager()
            .await
            .map_err(|e| CacheError::CacheAccessError(e.to_string()))?;

        let eviction_policy = config.eviction_policy.unwrap_or_default();
        Ok(Self {
            connection_manager,
            eviction_policy,
        })
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
        let mut connection = self.connection_manager.clone();
        let old = connection
            .hget(namespace, key)
            .await
            .map_err(|e| CacheError::GetError(e.to_string()))?;

        // Check if we need to evict an entry. If so, do it to make space for the upcoming insertion.
        // Note - We do this in an "optimistic" way, without locking anything.
        match self.eviction_policy {
            EvictionPolicy::NoEviction => {}
            EvictionPolicy::RandomEviction(max_entries) => {
                // Count items
                let count: usize = connection
                    .hlen(namespace)
                    .await
                    .map_err(|e| CacheError::GetError(e.to_string()))?;
                if count >= max_entries {
                    // Pull all the entries and take one at random
                    let entries: Vec<String> = connection
                        .hkeys(namespace)
                        .await
                        .map_err(|e| CacheError::GetError(e.to_string()))?;
                    if let Some(evict) = entries.get(random_usize(0, entries.len()).unwrap_or(0)) {
                        let () = connection
                            .hdel(namespace, evict)
                            .await
                            .map_err(|e| CacheError::PutError(e.to_string()))?;
                    }
                }
            }
        }

        let () = connection
            .hset(namespace, key, value)
            .await
            .map_err(|e| CacheError::PutError(e.to_string()))?;

        Ok(old)
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, CacheError> {
        let mut connection = self.connection_manager.clone();
        let r = connection
            .hget(namespace, key)
            .await
            .map_err(|e| CacheError::GetError(e.to_string()))?;
        Ok(r)
    }
}

/// Return a random `usize` in the half-open range `[start, end)`
/// using simple modulo reduction (may introduce slight bias).
fn random_usize(start: usize, end: usize) -> Result<usize, ()> {
    let rng = ring::rand::SystemRandom::new();
    let mut buf = [0u8; 8];
    // We'll pull 8 bytes (u64) and cast down to usize.
    rng.fill(&mut buf).map_err(|_| ())?;
    let n = u64::from_be_bytes(buf) as usize;

    let span = end.checked_sub(start).ok_or(())?;
    Ok(start + (n % span))
}
