//! This module provides a way for Plaid to use AWS DynamoDB as a DB for persistent storage.

use std::collections::HashMap;

use async_trait::async_trait;

use aws_sdk_dynamodb::{
    types::{AttributeValue, KeyType},
    Client,
};
use serde::Deserialize;

use crate::{get_aws_sdk_config, AwsAuthentication};

use super::{StorageError, StorageProvider};

const NAMESPACE: &str = "namespace";
const KEY: &str = "key";
const VALUE: &str = "value";

/// Configuration for DynamoDB
#[derive(Deserialize)]
pub struct Config {
    /// How to authenticate to AWS
    pub authentication: AwsAuthentication,
    /// The name of DynamoDB table used for Plaid's DB
    pub table_name: String,
}

/// A wrapper for DynamoDB
pub struct DynamoDb {
    client: Client,
    table_name: String,
}

impl DynamoDb {
    pub async fn new(config: Config) -> Result<Self, String> {
        let sdk_config = get_aws_sdk_config(config.authentication).await;
        let client = aws_sdk_dynamodb::Client::new(&sdk_config);

        // Perform schema validation
        Self::validate_schema(&client, &config.table_name).await?;

        Ok(Self {
            client,
            table_name: config.table_name,
        })
    }

    /// Validate the DynamoDB table's schema to ensure it is the expected one.
    /// The configured table must have
    /// * A partition key called "namespace"
    /// * A sort key called "key"
    async fn validate_schema(client: &Client, table_name: &str) -> Result<(), String> {
        let resp = client
            .describe_table()
            .table_name(table_name)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let table = resp.table().ok_or("Missing table metadata".to_string())?;

        let key_schema = table.key_schema();
        let partition_key = key_schema.iter().find(|k| k.key_type() == &KeyType::Hash);
        let sort_key = key_schema.iter().find(|k| k.key_type() == &KeyType::Range);

        if partition_key.map(|k| k.attribute_name()) != Some(NAMESPACE) {
            return Err("Invalid name for partition key".to_string());
        }

        if sort_key.map(|k| k.attribute_name()) != Some(KEY) {
            return Err("Invalid name for sort key".to_string());
        }

        Ok(())
    }
}

#[async_trait]
impl StorageProvider for DynamoDb {
    fn is_persistent(&self) -> bool {
        true
    }

    async fn insert(
        &self,
        namespace: String,
        key: String,
        value: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let response = self
            .client
            .put_item()
            .table_name(&self.table_name)
            .item(NAMESPACE, AttributeValue::S(namespace))
            .item(KEY, AttributeValue::S(key))
            .item(VALUE, AttributeValue::B(value.into()))
            .return_values(aws_sdk_dynamodb::types::ReturnValue::AllOld)
            .send()
            .await
            .map_err(|e| StorageError::Access(format!("Could not insert to storage: {e}")))?;
        // Return the previous entry, if any
        Ok(response
            .attributes
            .and_then(|attr| Some(attr.get(VALUE)?.as_b().ok()?.clone()))
            .and_then(|v| Some(v.into_inner())))
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let response = self
            .client
            .get_item()
            .consistent_read(true)
            .table_name(&self.table_name)
            .key(NAMESPACE, AttributeValue::S(namespace.to_string()))
            .key(KEY, AttributeValue::S(key.to_string()))
            .send()
            .await
            .map_err(|e| StorageError::Access(format!("Could not get from storage: {e}")))?;
        Ok(response
            .item
            .and_then(|attr| Some(attr.get(VALUE)?.as_b().ok()?.clone()))
            .and_then(|v| Some(v.into_inner())))
    }

    async fn delete(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let response = self
            .client
            .delete_item()
            .table_name(&self.table_name)
            .key(NAMESPACE, AttributeValue::S(namespace.to_string()))
            .key(KEY, AttributeValue::S(key.to_string()))
            .return_values(aws_sdk_dynamodb::types::ReturnValue::AllOld)
            .send()
            .await
            .map_err(|e| StorageError::Access(format!("Could not delete from storage: {e}")))?;
        Ok(response
            .attributes
            .and_then(|attr| Some(attr.get(VALUE)?.as_b().ok()?.clone()))
            .and_then(|v| Some(v.into_inner())))
    }

    async fn list_keys(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, StorageError> {
        let mut all_keys = vec![];

        let items = dynamodb_query(
            &self.client,
            &self.table_name,
            namespace,
            prefix,
            vec![KEY].as_slice(),
        )
        .await?;

        // Add retrieved items to our growing list
        for item in &items {
            all_keys.push(
                item.get(KEY)
                    .ok_or(StorageError::Access(
                        "Could not list storage contents".to_string(),
                    ))?
                    .as_s()
                    .map_err(|_| {
                        StorageError::Access("Could not list storage contents".to_string())
                    })?
                    .clone(),
            );
        }

        Ok(all_keys)
    }

    async fn fetch_all(
        &self,
        namespace: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>, StorageError> {
        let mut everything = vec![];

        let items = dynamodb_query(
            &self.client,
            &self.table_name,
            namespace,
            prefix,
            vec!["key", "value"].as_slice(),
        )
        .await?;

        // Add retrieved items to our growing list
        for item in &items {
            everything.push((
                item.get(KEY)
                    .ok_or(StorageError::Access(
                        "Could not list storage contents".to_string(),
                    ))?
                    .as_s()
                    .map_err(|_| {
                        StorageError::Access("Could not list storage contents".to_string())
                    })?
                    .clone(),
                item.get(VALUE)
                    .ok_or(StorageError::Access(
                        "Could not list storage contents".to_string(),
                    ))?
                    .as_b()
                    .map_err(|_| {
                        StorageError::Access("Could not list storage contents".to_string())
                    })?
                    .clone()
                    .into_inner(),
            ));
        }

        Ok(everything)
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

/// Perform a query on DynamoDB.
///
/// Args:  
/// `client` - a DynamoDB Client  
/// `table_name` - the name of the DynamoDB table  
/// `namespace` - the namespace we want to query  
/// `prefix` - [Optional] a prefix that keys must have in order to be returned by this query  
/// `attributes_to_get` - A list of attributes to get from DynamoDB
async fn dynamodb_query(
    client: &Client,
    table_name: &str,
    namespace: &str,
    prefix: Option<&str>,
    attributes_to_get: &[&str],
) -> Result<Vec<HashMap<String, AttributeValue>>, StorageError> {
    // This is necessary because the STL is converting a None prefix to an empty string,
    // which would result in passing an empty string to DynamoDB, which then complains.
    // So here we re-convert an empty string to a None prefix.
    let prefix = match prefix {
        Some(x) if !x.is_empty() => Some(x),
        _ => None,
    };

    let mut output: Vec<HashMap<String, AttributeValue>> = vec![];

    // For pagination
    let mut last_evaluated_key: Option<HashMap<String, AttributeValue>> = None;

    // Prepare the projection expression.
    // Alias all attributes by prepending with a #, to address the case where the attribute name is a reserved DynamoDB keyword
    let aliased: Vec<String> = attributes_to_get.iter().map(|v| format!("#{v}")).collect();
    // Now that everything is aliased, we can have a "safe" projection expression
    let projection_expression = aliased.join(",");

    // Start filling in the mappings for names and attributes (to go back from aliased to real names)
    let mut expression_attribute_names = vec![];
    for (index, attribute) in attributes_to_get.iter().enumerate() {
        expression_attribute_names.push((aliased[index].to_string(), attribute.to_string()));
    }
    expression_attribute_names.push(("#pk".to_string(), NAMESPACE.to_string()));

    let mut expression_attribute_values = vec![];
    expression_attribute_values.push((
        ":pk_val".to_string(),
        AttributeValue::S(namespace.to_string()),
    ));

    // Prepare the key condition expression, with the side-effect of adding items to the aliased->real mapping, if necessary
    let key_condition_expression = match prefix {
        None => "#pk = :pk_val",
        Some(prefix) => {
            expression_attribute_names.push(("#sk".to_string(), KEY.to_string()));
            expression_attribute_values.push((
                ":sk_prefix".to_string(),
                AttributeValue::S(prefix.to_string()),
            ));
            "#pk = :pk_val AND begins_with(#sk, :sk_prefix)"
        }
    }
    .to_string();

    loop {
        let mut request = client
            .query()
            .consistent_read(true)
            .table_name(table_name)
            .projection_expression(projection_expression.clone());

        // Set in the request the key condition expression, expression attribute names and expression attribute values
        // that we had prepared above
        request = request
            .set_key_condition_expression(Some(key_condition_expression.clone()))
            .set_expression_attribute_names(Some(
                expression_attribute_names.clone().into_iter().collect(),
            ))
            .set_expression_attribute_values(Some(
                expression_attribute_values.clone().into_iter().collect(),
            ));

        // If we are continuing a previous query, start from where we had left off
        if let Some(lek) = last_evaluated_key {
            request = request.set_exclusive_start_key(Some(lek));
        }

        let response = request.send().await.map_err(|e| {
            let raw_response = match e.raw_response() {
                Some(resp) => format!("{:?}", resp),
                None => "raw response not available".to_string(),
            };
            StorageError::Access(format!(
                "Could not list storage contents: {e} {raw_response}"
            ))
        })?;

        // Add retrieved items to our growing list
        if let Some(items) = response.items {
            output.extend(items);
        }

        // Set up for next iteration
        last_evaluated_key = response.last_evaluated_key;

        // Stop when there's no more data
        if last_evaluated_key.is_none() {
            break;
        }
    }

    Ok(output)
}
