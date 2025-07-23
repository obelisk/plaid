#[cfg(feature = "redis")]
pub mod redis;

pub mod in_memory;

use std::fmt::Display;

use async_trait::async_trait;
use serde::Deserialize;

use crate::cache::in_memory::InMemoryCache;

#[derive(Deserialize, Clone)]
#[serde(tag = "type")]
pub enum CacheBackend {
    InMemory,
    #[cfg(feature = "redis")]
    Redis(redis::Config),
}

#[derive(Deserialize, Clone)]
pub struct Config {
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
    CouldNotAccessCache(String),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CacheInitError(ref e) => write!(f, "Error while initializing cache: {e}"),
            Self::NoCacheConfigured => {
                write!(f, "No cache system configuration could be found")
            }
            Self::CouldNotAccessCache(ref e) => {
                write!(f, "Access to the cache datastore was not possible: {e}")
            }
        }
    }
}

impl std::error::Error for CacheError {}

/// Defines the basic methods that all cache providers must offer.
#[async_trait]
pub trait CacheProvider {
    async fn put(&mut self, key: &str, value: &str) -> Result<Option<String>, CacheError>;
    async fn get(&mut self, key: &str) -> Result<Option<String>, CacheError>;
}

impl Cache {
    pub async fn new(capacity: usize, config: Config) -> Result<Self, CacheError> {
        let cache: Box<dyn CacheProvider + Send + Sync> = match config.backend {
            Some(CacheBackend::InMemory) => Box::new(InMemoryCache::new(capacity)),
            // Some(...) for Redis
            _ => return Err(CacheError::NoCacheConfigured),
        };

        Ok(Cache { cache })
    }

    pub async fn put(
        &mut self,
        key: impl Display,
        value: impl Display,
    ) -> Result<Option<String>, CacheError> {
        self.cache.put(&key.to_string(), &value.to_string()).await
    }

    pub async fn get(&mut self, key: impl Display) -> Result<Option<String>, CacheError> {
        self.cache.get(&key.to_string()).await
    }
}
