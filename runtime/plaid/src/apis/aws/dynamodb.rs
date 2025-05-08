use aws_sdk_dynamodb::primitives::Blob;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
use aws_sdk_dynamodb::Client;
use serde_json::{json, Map, Value};

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use serde::Deserialize;

use crate::{get_aws_sdk_config, loader::PlaidModule, AwsAuthentication};

/// Defines configuration for the KMS API
#[derive(Deserialize)]
pub struct DynamoDbConfig {
    /// This can either be:
    /// - `IAM`: Uses the IAM role assigned to the instance or environment.
    /// - `ApiKey`: Uses explicit credentials, including an access key ID, secret access key, and region.
    authentication: AwsAuthentication,
    /// Configured writers - maps a table name to a list of rules that are allowed to write it
    write: HashMap<String, HashSet<String>>,
    /// Configured readers - maps a table name to a list of rules that are allowed to read it
    read: HashMap<String, HashSet<String>>,
}

/// Represents the KMS API that handles all requests to KMS
pub struct DynamoDb {
    /// The underlying KMS client used to interact with the KMS API.
    client: Client,
    /// Configured writers - maps a table name to a list of rules that are allowed to write it
    write: HashMap<String, HashSet<String>>,
    /// Configured readers - maps a table name to a list of rules that are allowed to read it
    read: HashMap<String, HashSet<String>>,
}

impl DynamoDb {
    /// Creates a new instance of `Kms`
    pub async fn new(config: DynamoDbConfig) -> Self {
        let DynamoDbConfig {
            authentication,
            write,
            read,
        } = config;
        let sdk_config = get_aws_sdk_config(authentication).await;
        let client = Client::new(&sdk_config);

        Self {
            client,
            write,
            read,
        }
    }

    // PutItem function with direct deserialization
    pub async fn put_item(
        &self,
        module: Arc<PlaidModule>,
        table_name: &str,
        item_json: Value,
        expression_attribute_names_json: Option<Value>,
        expression_attribute_values_json: Option<Value>,
        condition_expression: Option<&str>,
        return_values: Option<&str>,
    ) -> Result<Option<Value>, String> {
        if let Some(write_access) = self.write.get(table_name) {
            // check if this module has write access to this table
            if !write_access.contains(&module.to_string()) {
                return Err(format!("{module} no write access for {table_name}"));
            }
        } else {
            return Err(format!("{module} no write access for {table_name}"));
        };
        // map item
        let item_map: HashMap<String, Value> = serde_json::from_value(item_json)
            .map_err(|e| format!("Failed to deserialize item JSON: {}", e))?;

        let mut dynamo_item = HashMap::<String, AttributeValue>::new();
        for (key, value) in item_map {
            let attr_value = json_to_attribute_value(value)?;
            dynamo_item.insert(key, attr_value);
        }

        // expression attribute names
        let dynamo_expression_attribute_names =
            if let Some(expr_names_json) = expression_attribute_names_json {
                let expression_names_map: HashMap<String, String> =
                    serde_json::from_value(expr_names_json).map_err(|e| {
                        format!(
                            "Failed to deserialize expression_attribute_values_json JSON: {}",
                            e
                        )
                    })?;

                Some(expression_names_map)
            } else {
                None
            };

        // expression attribute values
        let dynamo_expression_attribute_values =
            if let Some(expr_vals_json) = expression_attribute_values_json {
                let expression_vals_map: HashMap<String, Value> =
                    serde_json::from_value(expr_vals_json).map_err(|e| {
                        format!(
                            "Failed to deserialize expression_attribute_values_json JSON: {}",
                            e
                        )
                    })?;

                let mut express_attribute_values = HashMap::<String, AttributeValue>::new();
                for (key, value) in expression_vals_map {
                    let attr_value = json_to_attribute_value(value)?;
                    express_attribute_values.insert(key, attr_value);
                }
                Some(express_attribute_values)
            } else {
                None
            };

        // Parse return_values (if provided)
        let return_values = return_values
            .as_ref()
            .map(|rv| match *rv {
                "ALL_OLD" => Ok(ReturnValue::AllOld),
                "NONE" | "" => Ok(ReturnValue::None),
                _ => Err(format!(
                    "Invalid return_values: {}. Expected 'ALL_OLD' or 'NONE'",
                    rv
                )),
            })
            .transpose()?;

        // Execute PutItem
        let output = self
            .client
            .put_item()
            .table_name(table_name)
            .set_item(Some(dynamo_item))
            .set_expression_attribute_names(dynamo_expression_attribute_names)
            .set_expression_attribute_values(dynamo_expression_attribute_values)
            .set_condition_expression(condition_expression.map(|x| x.to_string()))
            .set_return_values(return_values)
            .send()
            .await
            .map_err(|e| format!("PutItem failed: {:?}", e))?;

        match output.attributes() {
            None => Ok(None),
            Some(attrs) => {
                // convert to json
                let mut result = Map::new();
                for (k, v) in attrs.iter() {
                    let new_val = attribute_value_to_json(v)?;
                    result.insert(k.clone(), new_val);
                }

                Ok(Some(Value::Object(result)))
            }
        }
    }

    pub async fn delete(
        &self,
        module: Arc<PlaidModule>,
        table_name: &str,
        key_json: Option<Value>,
        key_condition_expression: Option<&str>,
        expression_attribute_names_json: Option<Value>,
        expression_attribute_values_json: Option<Value>,
        return_values: Option<&str>,
    ) -> Result<Option<Value>, String> {
        if let Some(write_access) = self.write.get(table_name) {
            // check if this module has write access to this table
            if !write_access.contains(&module.to_string()) {
                return Err(format!("{module} no write access for {table_name}"));
            }
        } else {
            return Err(format!("{module} no write access for {table_name}"));
        };
        // expression attribute names
        let dynamo_expression_attribute_names =
            if let Some(expr_names_json) = expression_attribute_names_json {
                let expression_names_map: HashMap<String, String> =
                    serde_json::from_value(expr_names_json).map_err(|e| {
                        format!(
                            "Failed to deserialize expression_attribute_names_json JSON: {}",
                            e
                        )
                    })?;

                Some(expression_names_map)
            } else {
                None
            };

        // expression attribute values
        let dynamo_expression_attribute_values =
            if let Some(expr_vals_json) = expression_attribute_values_json {
                let expression_vals_map: HashMap<String, Value> =
                    serde_json::from_value(expr_vals_json).map_err(|e| {
                        format!(
                            "Failed to deserialize expression_attribute_values_json JSON: {}",
                            e
                        )
                    })?;

                let mut express_attribute_values = HashMap::<String, AttributeValue>::new();
                for (key, value) in expression_vals_map {
                    let attr_value = json_to_attribute_value(value)?;
                    express_attribute_values.insert(key, attr_value);
                }
                Some(express_attribute_values)
            } else {
                None
            };

        // expression attribute values
        let dynamo_key = if let Some(inner_key_json) = key_json {
            let key_map: HashMap<String, Value> = serde_json::from_value(inner_key_json)
                .map_err(|e| format!("Failed to deserialize key_json JSON: {}", e))?;

            let mut key_attrs = HashMap::<String, AttributeValue>::new();
            for (key, value) in key_map {
                let attr_value = json_to_attribute_value(value)?;
                key_attrs.insert(key, attr_value);
            }
            Some(key_attrs)
        } else {
            None
        };

        // Parse return_values (if provided)
        let return_values = return_values
            .as_ref()
            .map(|rv| match *rv {
                "ALL_OLD" => Ok(ReturnValue::AllOld),
                "NONE" | "" => Ok(ReturnValue::None),
                _ => Err(format!(
                    "Invalid return_values: {}. Expected 'ALL_OLD' or 'NONE'",
                    rv
                )),
            })
            .transpose()?;

        // Execute Query
        let output = self
            .client
            .delete_item()
            .table_name(table_name)
            .set_key(dynamo_key)
            .set_condition_expression(key_condition_expression.map(|x| x.to_string()))
            .set_expression_attribute_names(dynamo_expression_attribute_names)
            .set_expression_attribute_values(dynamo_expression_attribute_values)
            .set_return_values(return_values)
            .send()
            .await
            .map_err(|e| format!("Query failed: {:?}", e))?;

        // convert to json
        match output.attributes() {
            None => Ok(None),
            Some(attrs) => {
                // convert to json
                let mut result = Map::new();
                for (k, v) in attrs.iter() {
                    let new_val = attribute_value_to_json(v)?;
                    result.insert(k.clone(), new_val);
                }

                Ok(Some(Value::Object(result)))
            }
        }
    }

    pub async fn query(
        &self,
        module: Arc<PlaidModule>,
        table_name: &str,
        index_name: Option<&str>,
        key_condition_expression: &str,
        expression_attribute_names_json: Option<Value>,
        expression_attribute_values_json: Option<Value>,
    ) -> Result<Vec<Value>, String> {
        if let Some(read_access) = self.read.get(table_name) {
            // check if this module has read access to this table
            if !read_access.contains(&module.to_string()) {
                return Err(format!("{module} no write access for {table_name}",));
            }
        } else {
            return Err(format!("{module} no read access for {table_name}"));
        };
        // expression attribute names
        let dynamo_expression_attribute_names =
            if let Some(expr_names_json) = expression_attribute_names_json {
                let expression_names_map: HashMap<String, String> =
                    serde_json::from_value(expr_names_json).map_err(|e| {
                        format!(
                            "Failed to deserialize expression_attribute_values_json JSON: {}",
                            e
                        )
                    })?;

                Some(expression_names_map)
            } else {
                None
            };

        // expression attribute values
        let dynamo_expression_attribute_values =
            if let Some(expr_vals_json) = expression_attribute_values_json {
                let expression_vals_map: HashMap<String, Value> =
                    serde_json::from_value(expr_vals_json).map_err(|e| {
                        format!(
                            "Failed to deserialize expression_attribute_values_json JSON: {}",
                            e
                        )
                    })?;

                let mut express_attribute_values = HashMap::<String, AttributeValue>::new();
                for (key, value) in expression_vals_map {
                    let attr_value = json_to_attribute_value(value)?;
                    express_attribute_values.insert(key, attr_value);
                }
                Some(express_attribute_values)
            } else {
                None
            };

        // Execute Query
        let output = self
            .client
            .query()
            .table_name(table_name)
            .set_index_name(index_name.map(|x| x.to_string()))
            .key_condition_expression(key_condition_expression)
            .set_expression_attribute_names(dynamo_expression_attribute_names)
            .set_expression_attribute_values(dynamo_expression_attribute_values)
            .send()
            .await
            .map_err(|e| format!("Query failed: {:?}", e))?;

        // convert to json
        let mut out: Vec<Value> = vec![];
        for item in output.items() {
            let mut result = Map::new();
            for (k, v) in item.iter() {
                let new_val = attribute_value_to_json(v)?;
                result.insert(k.clone(), new_val);
            }
            out.push(Value::Object(result))
        }

        Ok(out)
    }
}

// helper function to convert JSON Value to DynamoDB AttributeValue, supporting all types
fn json_to_attribute_value(value: Value) -> Result<AttributeValue, String> {
    match value {
        // String: Direct string value
        Value::String(s) => Ok(AttributeValue::S(s)),

        // Number: Convert to string for DynamoDB
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(AttributeValue::N(i.to_string()))
            } else if let Some(f) = n.as_f64() {
                Ok(AttributeValue::N(f.to_string()))
            } else {
                Err("Unsupported number format".to_string())
            }
        }

        // Boolean: Direct boolean value
        Value::Bool(b) => Ok(AttributeValue::Bool(b)),

        // Null: Direct null value
        Value::Null => Ok(AttributeValue::Null(true)),

        // Array: Handle lists and sets
        Value::Array(arr) => {
            if arr.is_empty() {
                return Err("Lists and sets cannot be empty".to_string());
            }
            // Check if all elements are strings (for SS)
            let all_strings = arr.iter().all(|v| v.is_string());
            // Check if all elements are numbers (for NS)
            let all_numbers = arr.iter().all(|v| v.is_number());
            // Check if all elements are binary (assuming base64-encoded strings for B)
            let all_binary = arr.iter().all(|v| {
                v.as_str()
                    .map(|s| base64::decode(s).is_ok())
                    .unwrap_or(false)
            });

            if all_strings {
                // String Set (SS)
                let strings: Vec<String> = arr
                    .into_iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect();
                Ok(AttributeValue::Ss(strings))
            } else if all_numbers {
                // Number Set (NS)
                let numbers: Result<Vec<String>, String> = arr
                    .into_iter()
                    .map(|v| {
                        if let Some(i) = v.as_i64() {
                            Ok(i.to_string())
                        } else if let Some(f) = v.as_f64() {
                            Ok(f.to_string())
                        } else {
                            Err("Invalid number in number set".to_string())
                        }
                    })
                    .collect();
                Ok(AttributeValue::Ns(numbers?))
            } else if all_binary {
                // Binary Set (BS)
                let binaries: Result<Vec<Blob>, String> = arr
                    .into_iter()
                    .map(|v| {
                        let s = v.as_str().ok_or("Invalid binary value".to_string())?;
                        let decoded = base64::decode(s)
                            .map_err(|e| format!("Failed to decode base64: {}", e))?;
                        Ok(Blob::new(decoded))
                    })
                    .collect();
                Ok(AttributeValue::Bs(binaries?))
            } else {
                // List (L)
                let items: Result<Vec<AttributeValue>, String> =
                    arr.into_iter().map(json_to_attribute_value).collect();
                Ok(AttributeValue::L(items?))
            }
        }

        // Object: Handle maps and binary values
        Value::Object(obj) => {
            // Check if the object represents a binary value (e.g., {"_binary": "base64string"})
            if obj.len() == 1 && obj.contains_key("_binary") {
                if let Some(Value::String(base64_str)) = obj.get("_binary") {
                    let decoded = base64::decode(base64_str)
                        .map_err(|e| format!("Failed to decode base64: {}", e))?;
                    return Ok(AttributeValue::B(Blob::new(decoded)));
                } else {
                    return Err("_binary must be a base64-encoded string".to_string());
                }
            }

            // Otherwise, treat as a map (M)
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(k, json_to_attribute_value(v)?);
            }
            Ok(AttributeValue::M(map))
        }
    }
}

// helper function to convert DynamoDB AttributeValue to JSON Value
fn attribute_value_to_json(attr_value: &AttributeValue) -> Result<Value, String> {
    match attr_value {
        AttributeValue::S(s) => Ok(Value::String(s.clone())),
        AttributeValue::N(n) => {
            // Try parsing as integer first, then float
            if let Ok(int_val) = n.parse::<i64>() {
                Ok(json!(int_val))
            } else if let Ok(float_val) = n.parse::<f64>() {
                Ok(json!(float_val))
            } else {
                Err(format!("Invalid number format: {}", n))
            }
        }
        AttributeValue::B(blob) => {
            let base64_str = base64::encode(blob.as_ref());
            Ok(json!({ "_binary": base64_str }))
        }
        AttributeValue::Bool(b) => Ok(Value::Bool(*b)),
        AttributeValue::Null(_) => Ok(Value::Null),
        AttributeValue::L(list) => {
            let json_list: Result<Vec<Value>, String> =
                list.iter().map(attribute_value_to_json).collect();
            Ok(Value::Array(json_list?))
        }
        AttributeValue::M(map) => {
            let mut json_map = serde_json::Map::new();
            for (key, value) in map {
                let json_value = attribute_value_to_json(value)?;
                json_map.insert(key.clone(), json_value);
            }
            Ok(Value::Object(json_map))
        }
        AttributeValue::Ss(strings) => {
            if strings.is_empty() {
                return Err("String set cannot be empty".to_string());
            }
            Ok(Value::Array(
                strings.iter().map(|s| Value::String(s.clone())).collect(),
            ))
        }
        AttributeValue::Ns(numbers) => {
            if numbers.is_empty() {
                return Err("Number set cannot be empty".to_string());
            }
            let json_numbers: Result<Vec<Value>, String> = numbers
                .iter()
                .map(|n| {
                    if let Ok(int_val) = n.parse::<i64>() {
                        Ok(json!(int_val))
                    } else if let Ok(float_val) = n.parse::<f64>() {
                        Ok(json!(float_val))
                    } else {
                        Err(format!("Invalid number in number set: {}", n))
                    }
                })
                .collect();
            Ok(Value::Array(json_numbers?))
        }
        AttributeValue::Bs(blobs) => {
            if blobs.is_empty() {
                return Err("Binary set cannot be empty".to_string());
            }
            let json_binaries: Vec<Value> = blobs
                .iter()
                .map(|blob| Value::String(base64::encode(blob.as_ref())))
                .collect();
            Ok(Value::Array(json_binaries))
        }
        // Handle any unexpected variants
        _ => Err(format!(
            "Unsupported AttributeValue variant: {:?}",
            attr_value
        )),
    }
}
