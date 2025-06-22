use aws_sdk_dynamodb::primitives::Blob;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
use aws_sdk_dynamodb::Client;
use plaid_stl::aws::dynamodb::{
    DeleteItemInput, DeleteItemOutput, PutItemInput, PutItemOutput, QueryInput, QueryOutput,
};
use serde_json::{json, Map, Value};

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use serde::Deserialize;

use crate::apis::ApiError;
use crate::{get_aws_sdk_config, loader::PlaidModule, AwsAuthentication};

/// Defines configuration for the DynamoDB API
#[derive(Deserialize)]
pub struct DynamoDbConfig {
    /// This can either be:
    /// - `IAM`: Uses the IAM role assigned to the instance or environment.
    /// - `ApiKey`: Uses explicit credentials, including an access key ID, secret access key, and region.
    authentication: AwsAuthentication,
    /// Configured writers - maps a table name to a list of rules that are allowed to READ or WRITE it
    write: HashMap<String, HashSet<String>>,
    /// Configured readers - maps a table name to a list of rules that are allowed to read it
    read: HashMap<String, HashSet<String>>,
}

/// Represents the DynamoDB API client.
/// NOTE: if Plaid is configured with the DynamoDB database backend, sharing tables here will lead to undefined behaviour
pub struct DynamoDb {
    /// The underlying KMS client used to interact with the KMS API.
    client: Client,
    /// Configured writers - maps a table name to a list of rules that are allowed to write it
    write: HashMap<String, HashSet<String>>,
    /// Configured readers - maps a table name to a list of rules that are allowed to read it
    read: HashMap<String, HashSet<String>>,
}

enum AccessScope {
    Read,
    Write,
}

impl DynamoDb {
    /// Creates a new instance of `DynamoDb`
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

    fn allow_operation(
        &self,
        access_scope: AccessScope,
        module: Arc<PlaidModule>,
        table_name: &str,
    ) -> Result<(), ApiError> {
        match access_scope {
            AccessScope::Read => {
                if let Some(read_access) = self.read.get(table_name) {
                    // check if this module has read access to this table
                    if !read_access.contains(&module.to_string()) {
                        return Err(ApiError::BadRequest);
                    }
                    Ok(())
                } else if let Some(write_access) = self.write.get(table_name) {
                    // check if this module has write access to this table
                    if !write_access.contains(&module.to_string()) {
                        return Err(ApiError::BadRequest);
                    }
                    Ok(())
                } else {
                    Err(ApiError::BadRequest)
                }
            }
            AccessScope::Write => {
                if module.test_mode {
                    return Err(ApiError::TestMode);
                }
                if let Some(write_access) = self.write.get(table_name) {
                    // check if this module has write access to this table
                    if !write_access.contains(&module.to_string()) {
                        return Err(ApiError::BadRequest);
                    }
                    Ok(())
                } else {
                    Err(ApiError::BadRequest)
                }
            }
        }
    }

    pub async fn put_item(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let PutItemInput {
            table_name,
            item,
            expression_attribute_names,
            expression_attribute_values,
            condition_expression,
            return_values,
        } = serde_json::from_str(params).map_err(|err| ApiError::SerdeError(err.to_string()))?;
        self.allow_operation(AccessScope::Write, module, &table_name)?;
        let item = map_json_to_attributes(Some(item))?;
        let expression_attribute_values = map_json_to_attributes(expression_attribute_values)?;
        let return_values = map_return_values(return_values)?;

        // Execute PutItem
        let output = self
            .client
            .put_item()
            .table_name(table_name)
            .set_item(item)
            .set_expression_attribute_names(expression_attribute_names)
            .set_expression_attribute_values(expression_attribute_values)
            .set_condition_expression(condition_expression.map(|x| x.to_string()))
            .set_return_values(return_values)
            .send()
            .await
            .map_err(|e| ApiError::DynamoDbPutItemError(e))?;

        let attributes = match output.attributes() {
            None => None,
            Some(attrs) => {
                let result = map_attributes_to_json(attrs)?;
                Some(result)
            }
        };

        let out = PutItemOutput { attributes };
        serde_json::to_string(&out).map_err(|err| ApiError::SerdeError(err.to_string()))
    }

    pub async fn delete_item(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let DeleteItemInput {
            table_name,
            key,
            key_condition_expression,
            expression_attribute_names,
            expression_attribute_values,
            return_values,
        } = serde_json::from_str(params).map_err(|err| ApiError::SerdeError(err.to_string()))?;
        self.allow_operation(AccessScope::Write, module, &table_name)?;
        let expression_attribute_values = map_json_to_attributes(expression_attribute_values)?;
        let dynamo_key = map_json_to_attributes(Some(key))?;
        let return_values = map_return_values(return_values)?;

        let output = self
            .client
            .delete_item()
            .table_name(table_name)
            .set_key(dynamo_key)
            .set_condition_expression(key_condition_expression)
            .set_expression_attribute_names(expression_attribute_names)
            .set_expression_attribute_values(expression_attribute_values)
            .set_return_values(return_values)
            .send()
            .await
            .map_err(|e| ApiError::DynamoDbDeleteItemError(e))?;

        let attributes = match output.attributes() {
            None => None,
            Some(attrs) => {
                let result = map_attributes_to_json(attrs)?;
                Some(result)
            }
        };

        let out = DeleteItemOutput { attributes };
        serde_json::to_string(&out).map_err(|err| ApiError::SerdeError(err.to_string()))
    }

    pub async fn query(&self, params: &str, module: Arc<PlaidModule>) -> Result<String, ApiError> {
        let QueryInput {
            table_name,
            index_name,
            key_condition_expression,
            expression_attribute_names,
            expression_attribute_values,
        } = serde_json::from_str(params).map_err(|err| ApiError::SerdeError(err.to_string()))?;
        self.allow_operation(AccessScope::Read, module, &table_name)?;
        let expression_attribute_values = map_json_to_attributes(expression_attribute_values)?;

        let output = self
            .client
            .query()
            .table_name(table_name)
            .set_index_name(index_name.map(|x| x.to_string()))
            .key_condition_expression(key_condition_expression)
            .set_expression_attribute_names(expression_attribute_names)
            .set_expression_attribute_values(expression_attribute_values)
            .send()
            .await
            .map_err(|e| ApiError::DynamoDbQueryError(e))?;

        // convert to json
        let mut out: QueryOutput = QueryOutput { items: vec![] };
        for item in output.items() {
            let result = map_attributes_to_json(item)?;
            out.items.push(result)
        }

        serde_json::to_string(&out).map_err(|err| ApiError::SerdeError(err.to_string()))
    }
}

fn map_attributes_to_json(attrs: &HashMap<String, AttributeValue>) -> Result<Value, ApiError> {
    let mut result = Map::new();
    for (k, v) in attrs.iter() {
        let new_val = attribute_value_to_json(v)?;
        result.insert(k.to_string(), new_val);
    }
    Ok(Value::Object(result))
}

fn map_json_to_attributes(
    value: Option<HashMap<String, Value>>,
) -> Result<Option<HashMap<String, AttributeValue>>, ApiError> {
    if let Some(expr_vals) = value {
        let mut express_attribute_values = HashMap::<String, AttributeValue>::new();
        for (key, value) in expr_vals {
            let attr_value = json_to_attribute_value(value)?;
            express_attribute_values.insert(key, attr_value);
        }
        Ok(Some(express_attribute_values))
    } else {
        Ok(None)
    }
}

fn map_return_values(value: Option<String>) -> Result<Option<ReturnValue>, ApiError> {
    value
        .as_ref()
        .map(|rv| match rv.as_str() {
            "ALL_NEW" => Ok(ReturnValue::AllNew),
            "ALL_OLD" => Ok(ReturnValue::AllOld),
            "UPDATED_NEW" => Ok(ReturnValue::UpdatedNew),
            "UPDATED_OLD" => Ok(ReturnValue::UpdatedOld),
            "NONE" | "" => Ok(ReturnValue::None),
            _ => Err(ApiError::SerdeError(format!(
                "Invalid return_values: {}.",
                rv
            ))),
        })
        .transpose()
}

// helper function to convert JSON Value to DynamoDB AttributeValue, supporting all types
fn json_to_attribute_value(value: Value) -> Result<AttributeValue, ApiError> {
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
                Err(ApiError::SerdeError(String::from(
                    "Unsupported number format",
                )))
            }
        }

        // Boolean: Direct boolean value
        Value::Bool(b) => Ok(AttributeValue::Bool(b)),

        // Null: Direct null value
        Value::Null => Ok(AttributeValue::Null(true)),

        // Array: Handle lists and sets
        Value::Array(arr) => {
            if arr.is_empty() {
                return Err(ApiError::SerdeError(String::from(
                    "Lists and sets cannot be empty",
                )));
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
                let numbers: Result<Vec<String>, ApiError> = arr
                    .into_iter()
                    .map(|v| {
                        if let Some(i) = v.as_i64() {
                            Ok(i.to_string())
                        } else if let Some(f) = v.as_f64() {
                            Ok(f.to_string())
                        } else {
                            Err(ApiError::SerdeError(String::from(
                                "Invalid number in number set",
                            )))
                        }
                    })
                    .collect();
                Ok(AttributeValue::Ns(numbers?))
            } else if all_binary {
                // Binary Set (BS)
                let binaries: Result<Vec<Blob>, ApiError> = arr
                    .into_iter()
                    .map(|v| {
                        let s = v
                            .as_str()
                            .ok_or(ApiError::SerdeError(String::from("Invalid binary value")))?;
                        let decoded = base64::decode(s).map_err(|e| {
                            ApiError::SerdeError(format!("Failed to decode base64: {}", e))
                        })?;
                        Ok(Blob::new(decoded))
                    })
                    .collect();
                Ok(AttributeValue::Bs(binaries?))
            } else {
                // List (L)
                let items: Result<Vec<AttributeValue>, ApiError> =
                    arr.into_iter().map(json_to_attribute_value).collect();
                Ok(AttributeValue::L(items?))
            }
        }

        // Object: Handle maps and binary values
        Value::Object(obj) => {
            // Check if the object represents a binary value (e.g., {"_binary": "base64string"})
            if obj.len() == 1 && obj.contains_key("_binary") {
                if let Some(Value::String(base64_str)) = obj.get("_binary") {
                    let decoded = base64::decode(base64_str).map_err(|e| {
                        ApiError::SerdeError(format!("Failed to decode base64: {}", e))
                    })?;
                    return Ok(AttributeValue::B(Blob::new(decoded)));
                } else {
                    return Err(ApiError::SerdeError(String::from(
                        "_binary must be a base64-encoded string",
                    )));
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
fn attribute_value_to_json(attr_value: &AttributeValue) -> Result<Value, ApiError> {
    match attr_value {
        AttributeValue::S(s) => Ok(Value::String(s.clone())),
        AttributeValue::N(n) => {
            // Try parsing as integer first, then float
            if let Ok(int_val) = n.parse::<i64>() {
                Ok(json!(int_val))
            } else if let Ok(float_val) = n.parse::<f64>() {
                Ok(json!(float_val))
            } else {
                Err(ApiError::SerdeError(format!(
                    "Invalid number format: {}",
                    n
                )))
            }
        }
        AttributeValue::B(blob) => {
            let base64_str = base64::encode(blob.as_ref());
            Ok(json!({ "_binary": base64_str }))
        }
        AttributeValue::Bool(b) => Ok(Value::Bool(*b)),
        AttributeValue::Null(_) => Ok(Value::Null),
        AttributeValue::L(list) => {
            let json_list: Result<Vec<Value>, ApiError> =
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
                return Err(ApiError::SerdeError(
                    "String set cannot be empty".to_string(),
                ));
            }
            Ok(Value::Array(
                strings.iter().map(|s| Value::String(s.clone())).collect(),
            ))
        }
        AttributeValue::Ns(numbers) => {
            if numbers.is_empty() {
                return Err(ApiError::SerdeError(
                    "Number set cannot be empty".to_string(),
                ));
            }
            let json_numbers: Result<Vec<Value>, ApiError> = numbers
                .iter()
                .map(|n| {
                    if let Ok(int_val) = n.parse::<i64>() {
                        Ok(json!(int_val))
                    } else if let Ok(float_val) = n.parse::<f64>() {
                        Ok(json!(float_val))
                    } else {
                        Err(ApiError::SerdeError(format!(
                            "Invalid number in number set: {}",
                            n
                        )))
                    }
                })
                .collect();
            Ok(Value::Array(json_numbers?))
        }
        AttributeValue::Bs(blobs) => {
            if blobs.is_empty() {
                return Err(ApiError::SerdeError(
                    "Binary set cannot be empty".to_string(),
                ));
            }
            let json_binaries: Vec<Value> = blobs
                .iter()
                .map(|blob| Value::String(base64::encode(blob.as_ref())))
                .collect();
            Ok(Value::Array(json_binaries))
        }
        // Handle any unexpected variants
        _ => Err(ApiError::SerdeError(format!(
            "Unsupported AttributeValue variant: {:?}",
            attr_value
        ))),
    }
}
