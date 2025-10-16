//! This module provides a way for Plaid to use Redis as a cache layer

use crate::cache::CacheError;
use async_trait::async_trait;
use redis::{aio::ConnectionManager, AsyncCommands};
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

        // Check if we need to evict an entry. If so, do it to make space for the upcoming insertion.
        // Note - We do this in an "optimistic" way, without locking anything.
        let old = match self.eviction_policy {
            EvictionPolicy::NoEviction => {
                let (old, ()): (Option<String>, ()) = redis::pipe()
                    .cmd("HGET")
                    .arg(namespace)
                    .arg(key)
                    .cmd("HSET")
                    .arg(namespace)
                    .arg(key)
                    .arg(value)
                    .query_async(&mut connection)
                    .await
                    .map_err(|e| CacheError::PutError(e.to_string()))?;

                old
            }
            EvictionPolicy::RandomEviction(max_entries) => {
                let script = redis::Script::new(
                    r#"
local ns  = KEYS[1]
local fld = ARGV[1]
local val = ARGV[2]
local max = tonumber(ARGV[3])

local old = redis.call('HGET', ns, fld)

if max and max > 0 then
  local count = redis.call('HLEN', ns)
  if count >= max then
    -- Redis 6.2+: pick a random field without pulling all keys
    local evict = redis.call('HRANDFIELD', ns)
    if evict then redis.call('HDEL', ns, evict) end
  end
end

redis.call('HSET', ns, fld, val)
return old
"#,
                );

                script
                    .key(namespace)
                    .arg(key)
                    .arg(value)
                    .arg(max_entries)
                    .invoke_async(&mut connection)
                    .await
                    .map_err(|e| CacheError::PutError(e.to_string()))?
            }
        };

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
