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
    /// Configured writers - maps a table name to a list of rules that are allowed to READ or WRITE data
    write: HashMap<String, HashSet<String>>,
    /// Configured readers - maps a table name to a list of rules that are allowed to READ data
    read: HashMap<String, HashSet<String>>,
}

/// Represents the DynamoDB API client.
/// NOTE: if Plaid is configured with the DynamoDB database backend, sharing tables here will lead to undefined behaviour
pub struct DynamoDb {
    /// The underlying KMS client used to interact with the KMS API.
    client: Client,
    /// Configured writers - maps a table name to a list of rules that are allowed to READ or WRITE data
    write: HashMap<String, HashSet<String>>,
    /// Configured readers - maps a table name to a list of rules that are allowed to READ data
    read: HashMap<String, HashSet<String>>,
}

#[derive(PartialEq, PartialOrd)]
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

    fn check_module_permissions(
        &self,
        access_scope: AccessScope,
        module: Arc<PlaidModule>,
        table_name: &str,
    ) -> Result<(), ApiError> {
        match access_scope {
            AccessScope::Read => {
                // check if read access is configured for this table
                if let Some(table_readers) = self.read.get(table_name) {
                    // check if this module has read access to this table
                    if table_readers.contains(&module.to_string()) {
                        return Ok(());
                    }
                }

                // check if write access is configured for this table
                // writers can also read
                if let Some(table_writers) = self.write.get(table_name) {
                    // check if this module has write access to this table
                    if table_writers.contains(&module.to_string()) {
                        return Ok(());
                    }
                }

                trace!(
                    "[{module}] failed [read] permission check for dynamodb table [{table_name}]"
                );
                Err(ApiError::BadRequest)
            }
            AccessScope::Write => {
                // check if write access is configured for this table
                if let Some(write_access) = self.write.get(table_name) {
                    // check if this module has write access to this table
                    if write_access.contains(&module.to_string()) {
                        return Ok(());
                    };
                }

                trace!(
                    "[{module}] failed [write] permission check for dynamodb table [{table_name}]"
                );
                Err(ApiError::BadRequest)
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
        self.check_module_permissions(AccessScope::Write, module, &table_name)?;
        let item = json_into_attributes(Some(item))?;
        let expression_attribute_values = json_into_attributes(expression_attribute_values)?;
        let return_values = return_value_from_string(return_values)?;

        println!("table_name {table_name}");
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
                let result = attributes_into_json(attrs)?;
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
            condition_expression: key_condition_expression,
            expression_attribute_names,
            expression_attribute_values,
            return_values,
        } = serde_json::from_str(params).map_err(|err| ApiError::SerdeError(err.to_string()))?;
        self.check_module_permissions(AccessScope::Write, module, &table_name)?;
        let expression_attribute_values = json_into_attributes(expression_attribute_values)?;
        let dynamo_key = json_into_attributes(Some(key))?;
        let return_values = return_value_from_string(return_values)?;

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
                let result = attributes_into_json(attrs)?;
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
        self.check_module_permissions(AccessScope::Read, module, &table_name)?;
        let expression_attribute_values = json_into_attributes(expression_attribute_values)?;

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
            let result = attributes_into_json(item)?;
            out.items.push(result)
        }

        serde_json::to_string(&out).map_err(|err| ApiError::SerdeError(err.to_string()))
    }
}

/// Converts DynamoDB Attributes into JSON
fn attributes_into_json(attrs: &HashMap<String, AttributeValue>) -> Result<Value, ApiError> {
    let mut result = Map::new();
    for (k, v) in attrs.iter() {
        let new_val = to_json_value(v)?;
        result.insert(k.to_string(), new_val);
    }
    Ok(Value::Object(result))
}

/// Converts JSON into DynamoDB Attributes
fn json_into_attributes(
    value: Option<HashMap<String, Value>>,
) -> Result<Option<HashMap<String, AttributeValue>>, ApiError> {
    if let Some(expr_vals) = value {
        let mut express_attribute_values = HashMap::<String, AttributeValue>::new();
        for (key, value) in expr_vals {
            let attr_value = to_attribute_value(value)?;
            express_attribute_values.insert(key, attr_value);
        }
        Ok(Some(express_attribute_values))
    } else {
        Ok(None)
    }
}

/// converts String into Typed ReturnValue enum for use with DynamoDB api
fn return_value_from_string(value: Option<String>) -> Result<Option<ReturnValue>, ApiError> {
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

enum ArrayMembers {
    AllStrings,
    AllNumbers,
    AllBinary,
    NonUniform,
}

/// helper function for use when converting JSON array to strongly typed array
fn inspect_array_members(arr: &Vec<Value>) -> ArrayMembers {
    // Check if all elements are strings (for SS)
    if arr.iter().all(|v| v.is_string()) {
        return ArrayMembers::AllStrings;
    }
    // Check if all elements are numbers (for NS)
    if arr.iter().all(|v| v.is_number()) {
        return ArrayMembers::AllNumbers;
    }
    // Check if all elements are binary (assuming base64-encoded strings for B)
    let all_binary = arr.iter().all(|v| {
        v.as_str()
            .map(|s| base64::decode(s).is_ok())
            .unwrap_or(false)
    });

    if all_binary {
        return ArrayMembers::AllBinary;
    }
    ArrayMembers::NonUniform
}

/// helper function to convert JSON Value to DynamoDB AttributeValue, supporting all types
fn to_attribute_value(value: Value) -> Result<AttributeValue, ApiError> {
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
            match inspect_array_members(&arr) {
                ArrayMembers::AllStrings => {
                    // String Set (SS)
                    let strings: Vec<String> = arr
                        .into_iter()
                        .map(|v| v.as_str().unwrap().to_string())
                        .collect();
                    Ok(AttributeValue::Ss(strings))
                }
                ArrayMembers::AllNumbers => {
                    // Number Set (NS)
                    let numbers: Result<Vec<String>, ApiError> = arr
                        .into_iter()
                        .map(|v| {
                            if let Some(i) = v.as_i64() {
                                Ok(i.to_string())
                            } else if let Some(f) = v.as_f64() {
                                Ok(f.to_string())
                            } else {
                                // Err("Invalid number in number set".to_string())
                                Err(ApiError::SerdeError(String::from(
                                    "Invalid number in number set",
                                )))
                            }
                        })
                        .collect();
                    Ok(AttributeValue::Ns(numbers?))
                }
                ArrayMembers::AllBinary => {
                    // Binary Set (BS)
                    let binaries: Result<Vec<Blob>, ApiError> = arr
                        .into_iter()
                        .map(|v| {
                            let s = v.as_str().ok_or(ApiError::SerdeError(String::from(
                                "Invalid binary value",
                            )))?;
                            let decoded = base64::decode(s).map_err(|e| {
                                ApiError::SerdeError(format!("Failed to decode base64: {}", e))
                            })?;
                            Ok(Blob::new(decoded))
                        })
                        .collect();
                    Ok(AttributeValue::Bs(binaries?))
                }
                ArrayMembers::NonUniform => {
                    // List (L)
                    let items: Result<Vec<AttributeValue>, ApiError> =
                        arr.into_iter().map(to_attribute_value).collect();
                    Ok(AttributeValue::L(items?))
                }
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
                map.insert(k, to_attribute_value(v)?);
            }
            Ok(AttributeValue::M(map))
        }
    }
}

// helper function to convert DynamoDB AttributeValue to JSON Value
fn to_json_value(attr_value: &AttributeValue) -> Result<Value, ApiError> {
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
            let json_list: Result<Vec<Value>, ApiError> = list.iter().map(to_json_value).collect();
            Ok(Value::Array(json_list?))
        }
        AttributeValue::M(map) => {
            let mut json_map = serde_json::Map::new();
            for (key, value) in map {
                let json_value = to_json_value(value)?;
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

#[cfg(test)]
pub mod tests {
    use aws_config::Region;
    use serde_json::from_value;
    use wasmer::{
        sys::{Cranelift, EngineBuilder},
        Module, Store,
    };

    use crate::loader::LimitValue;

    use super::*;

    // helper function to generate a blank module that does nothing
    fn test_module(name: &str, test_mode: bool) -> Arc<PlaidModule> {
        let store = Store::default();
        // stub wasm module, just enough to pass validation
        // https://docs.rs/wabt/latest/wabt/fn.wat2wasm.html
        let wasm = &[
            0, 97, 115, 109, // \0ASM - magic
            1, 0, 0, 0, //  0x01 - version
        ];
        let compiler_config = Cranelift::default();
        let engine = EngineBuilder::new(compiler_config);
        let m = Module::new(&store, wasm).unwrap();

        Arc::new(PlaidModule {
            name: name.to_string(),
            module: m,
            engine: engine.into(),
            computation_limit: 0,
            page_limit: 0,
            storage_current: Default::default(),
            storage_limit: LimitValue::Unlimited,
            accessory_data: Default::default(),
            secrets: Default::default(),
            cache: Default::default(),
            persistent_response: Default::default(),
            test_mode,
        })
    }

    impl DynamoDb {
        // constructor for the local instance of DynamoDB
        pub async fn local_endpoint(
            read: HashMap<String, HashSet<String>>,
            write: HashMap<String, HashSet<String>>,
        ) -> Self {
            let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
                .test_credentials()
                .region(Region::new("us-east-1"))
                // DynamoDB run locally uses port 8000 by default.
                .endpoint_url("http://dynamodb:8000")
                .load()
                .await;
            let dynamodb_local_config = aws_sdk_dynamodb::config::Builder::from(&config).build();

            let client = aws_sdk_dynamodb::Client::from_conf(dynamodb_local_config);

            Self {
                client,
                read,
                write,
            }
        }
    }

    #[tokio::test]
    async fn put_query_delete() {
        // Initialize the client
        let table_name = String::from("local_test");
        let writers = json!({table_name.clone(): ["test_module"]});
        let writers = from_value::<HashMap<String, HashSet<String>>>(writers).unwrap();
        let client = DynamoDb::local_endpoint(HashMap::new(), writers).await;
        // Example: PutItem with all attribute types
        // NOTE:: The json object represents an item
        // in a dynamodb table, each key is an attribute (column)
        // the primary + secondary keys must also be contained
        // in the object as keys
        let item_json = serde_json::json!({
            "pk": "124",
            "timestamp": "124",
            "name": "Jane Doe",
            "age": 33,
            "is_active": true,
            "null_field": null,
            "scores": [95, 88, 92], // List
            "metadata": { // Map
                "city": "New York",
                "country": "USA"
            },
            "tags": ["dev", "rust", "aws"], // String Set
            "ratings": [4.5, 3.8, 5.0], // Number Set
            "binaries": [ // Binary Set (base64-encoded)
                base64::encode("data1"),
                base64::encode("data2")
            ],
            "binary_field": { "_binary": base64::encode("binary_data") } // Binary
        });
        let item_hm = serde_json::from_value::<HashMap<String, Value>>(item_json).unwrap();
        let input = PutItemInput {
            table_name: table_name.clone(),
            item: item_hm,
            return_values: Some(String::from("ALL_OLD")),
            ..Default::default()
        };
        let input = serde_json::to_string(&input).unwrap();
        let m = test_module("test_module", true);
        let output = client.put_item(&input, m.clone()).await.unwrap();

        println!("put_item output {output:?}");

        let input = QueryInput {
            table_name: table_name.clone(),
            key_condition_expression: String::from("#pk = :val"),
            expression_attribute_names: Some(HashMap::from([(
                "#pk".to_string(),
                "pk".to_string(),
            )])),
            expression_attribute_values: Some(HashMap::from([(
                ":val".to_string(),
                Value::String(String::from("124")),
            )])),
            ..Default::default()
        };
        let input = serde_json::to_string(&input).unwrap();
        let output = client.query(&input, m.clone()).await.unwrap();

        println!(
            "query output {}",
            serde_json::to_string_pretty(&output).unwrap()
        );

        let input = DeleteItemInput {
            table_name: table_name,
            key: HashMap::from([
                (String::from("pk"), Value::String(String::from("124"))),
                (
                    String::from("timestamp"),
                    Value::String(String::from("124")),
                ),
            ]),
            return_values: Some(String::from("ALL_OLD")),
            ..Default::default()
        };

        let input = serde_json::to_string(&input).unwrap();
        let output = client.delete_item(&input, m.clone()).await;

        println!("delete {:?}", output);
    }
}
