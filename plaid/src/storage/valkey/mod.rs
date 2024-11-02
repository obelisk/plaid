use async_trait::async_trait;
use fred::{prelude::*, types::{ConnectHandle, Scanner}};

use futures_util::TryStreamExt;
use serde::Deserialize;

use super::{StorageError, StorageProvider};

#[derive(Deserialize)]
pub struct Config {
    pub connection_string: String,
}

pub struct Valkey {
    client: RedisClient,
    handle: ConnectHandle,
}

impl Valkey {
    pub fn new(config: Config) -> Result<Self, StorageError> {
        // Create a RedisConfig from a connection string
        let config = RedisConfig::from_url(&config.connection_string)
            .map_err(|e| StorageError::CouldNotAccessStorage(e.to_string()))?;
        // Build a client with it. We may add more features and configuration options here
        // in the future.
        let client = Builder::from_config(config)
            .build()
            .map_err(|e| StorageError::CouldNotAccessStorage(e.to_string()))?;

        // Initialize the client utilizing interior mutability
        let handle = client.connect();

        Ok(Self { client, handle})
    }
}

#[async_trait]
impl StorageProvider for Valkey {
    async fn insert(
        &self,
        namespace: String,
        key: String,
        value: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        if self.handle.is_finished() {
            return Err(StorageError::CouldNotAccessStorage("Connection gone away".to_string()));
        }

        // We need to cast everything to String in ValKey.
        // For now, if we get a non-string value we will return an
        // error.
        let value = String::from_utf8(value).map_err(|e|{
            error!("{namespace} tried to set {key} with a non-utf8 value: {e}");
            StorageError::Access(key.to_string())
        })?;

        // Set the new value inside the namespace and return the old string
        // This will error if the type is not String. That should only happen
        // if the database was manually modified since we only use String values.
        match self.client.set::<String, _, _>(format!("{namespace}:{key}"), value, None, None, true).await {
            Ok(value) => Ok(Some(value.as_bytes().to_vec())),
            Err(e) => {
                error!("Could not set [{key}] in [{namespace}]. Error: {e}");
                Err(StorageError::Access(key.to_string()))
            },
        }
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        if self.handle.is_finished() {
            return Err(StorageError::CouldNotAccessStorage("Connection gone away".to_string()));
        }

        match self.client.get::<String, _>(format!("{namespace}:{key}")).await
            .map_err(|_| StorageError::Access(key.to_string())) {
                Ok(value) => Ok(Some(value.as_bytes().to_vec())),
                Err(e) => {
                    error!("Could not get [{key}] in [{namespace}]. Error: {e}");
                    Err(StorageError::Access(key.to_string()))
                },
            }
    }


    async fn delete(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        if self.handle.is_finished() {
            return Err(StorageError::CouldNotAccessStorage("Connection gone away".to_string()));
        }

        let old_value = self.get(namespace, key).await?;

        // Set the new value inside the namespace and return the old string
        // This will error if the type is not String. That should only happen
        // if the database was manually modified since we only use String values.
        match self.client.del::<String, _>(format!("{namespace}:{key}")).await {
            Ok(_) => Ok(old_value),
            Err(e) => {
                error!("Could not set [{key}] in [{namespace}]. Error: {e}");
                Err(StorageError::CouldNotAccessStorage(e.to_string()))
            },
        }
    }

    async fn list_keys(&self, namespace: &str, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        if self.handle.is_finished() {
            return Err(StorageError::CouldNotAccessStorage("Connection gone away".to_string()));
        }

        let prefix = prefix.unwrap_or_default();
        let mut keys = vec![];

        // scan all keys in the namespace, with the given prefix, returning 50 keys per page
        let mut scan_stream = self.client.scan(format!("{namespace}:{prefix}*"), Some(50), None);
        while let Some(mut page) = scan_stream.try_next().await.map_err(|e| StorageError::CouldNotAccessStorage(e.to_string()))? {
            keys.extend(
                page
                .take_results()
                .unwrap_or_default()
                    .into_iter()
                    .filter_map(|key| key.as_str().map(|x| x.to_owned())
                )
            );

            // **important:** move on to the next page now that we're done reading the values
            let _ = page.next();
        }


        Ok(keys)
    }

    async fn fetch_all(&self, namespace: &str, prefix: Option<&str>) -> Result<Vec<(String, Vec<u8>)>, StorageError> {
        if self.handle.is_finished() {
            return Err(StorageError::CouldNotAccessStorage("Connection gone away".to_string()));
        }

        let prefix = prefix.unwrap_or_default();
        let mut processed_keys = vec![];

        // scan all keys in the namespace, with the given prefix, returning 50 keys per page
        let mut scan_stream = self.client.scan(format!("{namespace}:{prefix}*"), Some(50), None);
        while let Some(mut page) = scan_stream.try_next().await.map_err(|e| StorageError::CouldNotAccessStorage(e.to_string()))? {
            if let Some(keys) = page.take_results() {
                // create a client from the scan result, reusing the existing connection(s)
                let client = page.create_client();

                for key in keys.into_iter() {
                    let value: Vec<u8> = match client.get::<String, _>(&key).await {
                        Ok(v) => v.as_bytes().to_vec(),
                        Err(_e) => continue,
                    };

                    let key = match key.as_str() {
                        Some(s) => s.to_owned(),
                        None => continue,
                    };

                    processed_keys.push((key, value));
                }
            }

            // **important:** move on to the next page now that we're done reading the values
            let _ = page.next();
        }
        Ok(processed_keys)
    }
}