#[cfg(feature = "redis")]
pub mod redis;

pub mod in_memory;

use std::{collections::HashMap, fmt::Display};

use async_trait::async_trait;
use serde::Deserialize;

use crate::loader::LimitedAmount;

use in_memory::InMemoryCache;

#[cfg(feature = "redis")]
use redis::RedisCache;

#[derive(Deserialize, Clone)]
#[serde(tag = "type")]
pub enum CacheBackend {
    InMemory,
    #[cfg(feature = "redis")]
    Redis(redis::Config),
}

#[derive(Deserialize, Clone)]
pub struct Config {
    /// How many key-value entries a module is allowed to store in the cache.
    pub cache_entries: LimitedAmount,
    /// Which backend is used for the cache layer, if any.
    pub backend: Option<CacheBackend>,
}

/// The cache for Plaid
pub struct Cache {
    cache: Box<dyn CacheProvider + Send + Sync>,
}

/// Errors encountered while trying to use Plaid's cache.
#[derive(Debug)]
pub enum CacheError {
    CacheInitError(String),
    NoCacheConfigured,
    CacheAccessError(String),
    PutError(String),
    GetError(String),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CacheInitError(ref e) => write!(f, "Error while initializing cache: {e}"),
            Self::NoCacheConfigured => {
                write!(f, "No cache system configuration could be found")
            }
            Self::CacheAccessError(ref e) => {
                write!(f, "Could not access cache: {e}")
            }
            Self::PutError(ref e) => {
                write!(f, "Error while inserting into the cache: {e}")
            }
            Self::GetError(ref e) => {
                write!(f, "Error while getting from the cache: {e}")
            }
        }
    }
}

impl std::error::Error for CacheError {}

/// Defines the basic methods that all cache providers must offer.
#[async_trait]
pub trait CacheProvider {
    /// Insert a value in the cache. If this is overwriting a previous value, return the previous value.
    async fn put(
        &self,
        namespace: &str,
        key: &str,
        value: &str,
    ) -> Result<Option<String>, CacheError>;
    /// Get a value from the cache, if present.
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, CacheError>;
}

impl Cache {
    pub async fn new(
        modules_and_logtypes: HashMap<String, String>,
        config: Config,
    ) -> Result<Self, CacheError> {
        let cache: Box<dyn CacheProvider + Send + Sync> = match config.backend {
            Some(CacheBackend::InMemory) => {
                info!("Using in-memory cache");
                Box::new(InMemoryCache::new(modules_and_logtypes, config))
            }
            #[cfg(feature = "redis")]
            Some(CacheBackend::Redis(config)) => {
                // Note - capacity not taken into account when using redis
                info!("Using redis cache with config [{}]", config);
                Box::new(RedisCache::new(config).await.unwrap())
            }
            _ => return Err(CacheError::NoCacheConfigured),
        };

        Ok(Cache { cache })
    }

    pub async fn put(
        &self,
        namespace: impl Display,
        key: impl Display,
        value: impl Display,
    ) -> Result<Option<String>, CacheError> {
        self.cache
            .put(&namespace.to_string(), &key.to_string(), &value.to_string())
            .await
    }

    pub async fn get(
        &self,
        namespace: impl Display,
        key: impl Display,
    ) -> Result<Option<String>, CacheError> {
        self.cache
            .get(&namespace.to_string(), &key.to_string())
            .await
    }
}
