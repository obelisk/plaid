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

use serde::{Deserialize, Serialize};

use crate::apis::ApiError;
use crate::{get_aws_sdk_config, loader::PlaidModule, AwsAuthentication};

/// Defines configuration for the DynamoDB API
#[derive(Deserialize, Serialize)]
pub struct DynamoDbConfig {
    /// Enable dynamodb local endpoint for testing
    local_endpoint: bool,
    /// This can either be:
    /// - `IAM`: Uses the IAM role assigned to the instance or environment.
    /// - `ApiKey`: Uses explicit credentials, including an access key ID, secret access key, and region.
    #[serde(skip_serializing)]
    authentication: AwsAuthentication,
    /// Configured writers - maps a table name to a list of rules that are allowed to READ or WRITE data
    rw: HashMap<String, HashSet<String>>,
    /// Configured readers - maps a table name to a list of rules that are allowed to READ data
    r: HashMap<String, HashSet<String>>,
    /// Reserved tables - list of 'reserved' table names which rules cannot access
    /// For the purpose of preventing rules rule accessing 'storage' tables in
    #[serde(default)]
    reserved_tables: Option<HashSet<String>>,
}

/// Represents the DynamoDB API client.
/// NOTE: if Plaid is configured with the DynamoDB database backend, sharing tables here will lead to undefined behaviour
pub struct DynamoDb {
    /// The underlying client used to interact with the KMS API.
    client: Client,
    /// Configured writers - maps a table name to a list of rules that are allowed to READ or WRITE data
    rw: HashMap<String, HashSet<String>>,
    /// Configured readers - maps a table name to a list of rules that are allowed to READ data
    r: HashMap<String, HashSet<String>>,
    /// Reserved tables - list of 'reserved' table names which rules cannot access
    /// For the purpose of preventing rules rule accessing 'storage' tables in
    reserved_tables: Option<HashSet<String>>,
}

#[derive(PartialEq, PartialOrd)]
/// Represents an access scope for a rule has to modify a DynamoDB table
enum AccessScope {
    Read,
    Write,
}

impl DynamoDb {
    /// Creates a new instance of `DynamoDb`
    pub async fn new(config: DynamoDbConfig) -> Self {
        let DynamoDbConfig {
            authentication,
            rw,
            r,
            local_endpoint,
            reserved_tables,
        } = config;

        if local_endpoint {
            return DynamoDb::local_endpoint(r, rw, reserved_tables).await;
        }

        let sdk_config = get_aws_sdk_config(&authentication).await;
        let client = Client::new(&sdk_config);

        Self {
            client,
            rw,
            r,
            reserved_tables,
        }
    }

    /// Constructor for the local instance of DynamoDB
    async fn local_endpoint(
        r: HashMap<String, HashSet<String>>,
        rw: HashMap<String, HashSet<String>>,
        reserved_tables: Option<HashSet<String>>,
    ) -> Self {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            // DynamoDB run locally uses port 8000 by default.
            .endpoint_url("http://localhost:8000")
            .load()
            .await;
        let dynamodb_local_config = aws_sdk_dynamodb::config::Builder::from(&config).build();

        let client = aws_sdk_dynamodb::Client::from_conf(dynamodb_local_config);

        Self {
            client,
            r,
            rw,
            reserved_tables,
        }
    }

    /// Checks if module can perform given action
    /// TODO: need to check if table is a 'reserved_table'
    fn check_module_permissions(
        &self,
        access_scope: AccessScope,
        module: Arc<PlaidModule>,
        table_name: &str,
    ) -> Result<(), ApiError> {
        match access_scope {
            AccessScope::Read => {
                // check if read access is configured for this table
                if let Some(table_readers) = self.r.get(table_name) {
                    // check if this module has read access to this table
                    if table_readers.contains(&module.to_string()) {
                        return Ok(());
                    }
                }

                // check if write access is configured for this table
                // writers can also read
                if let Some(table_writers) = self.rw.get(table_name) {
                    // check if this module has write access to this table
                    if table_writers.contains(&module.to_string()) {
                        return Ok(());
                    }
                }

                warn!(
                    "[{module}] failed [read] permission check for dynamodb table [{table_name}]"
                );
                Err(ApiError::BadRequest)
            }
            AccessScope::Write => {
                // check if write access is configured for this table
                if let Some(write_access) = self.rw.get(table_name) {
                    // check if this module has write access to this table
                    if write_access.contains(&module.to_string()) {
                        return Ok(());
                    };
                }

                warn!(
                    "[{module}] failed [write] permission check for dynamodb table [{table_name}]"
                );
                Err(ApiError::BadRequest)
            }
        }
    }

    /// Creates a new item, or replaces an old item with a new item.
    /// If an item that has the same primary key as the new item already exists in the specified table,
    /// the new item completely replaces the existing item. You can perform a conditional put operation
    /// (add a new item if one with the specified primary key doesn't exist),
    /// or replace an existing item if it has certain attribute values.
    /// You can return the item's attribute values in the same operation, using the ReturnValues parameter.
    ///
    /// More Info:
    /// https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_PutItem.html
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

    /// Deletes a single item in a table by primary key. You can perform a conditional delete operation that deletes the item if it exists, or if it has an expected attribute value.
    /// In addition to deleting an item, you can also return the item's attribute values in the same operation, using the ReturnValues parameter.
    /// Unless you specify conditions, the DeleteItem is an idempotent operation; running it multiple times on the same item or attribute does not result in an error response.
    /// Conditional deletes are useful for deleting items only if specific conditions are met. If those conditions are met, DynamoDB performs the delete. Otherwise, the item is not deleted.
    ///
    /// More Info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_DeleteItem.html
    pub async fn delete_item(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let DeleteItemInput {
            table_name,
            key,
            condition_expression,
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
            .set_condition_expression(condition_expression)
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

    /// You must provide the name of the partition key attribute and a single value for that attribute.
    /// Query returns all items with that partition key value.
    /// Optionally, you can provide a sort key attribute and use a comparison operator to refine the search results.
    ///
    /// Use the KeyConditionExpression parameter to provide a specific value for the partition key.
    /// The Query operation will return all of the items from the table or index with that partition key value.
    ///
    /// More Info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_Query.html
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
    use serde_json::{from_str, from_value};
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
            logtype: "test".to_string(),
            module: m,
            engine: engine.into(),
            computation_limit: 0,
            page_limit: 0,
            storage_current: Default::default(),
            storage_limit: LimitValue::Unlimited,
            accessory_data: Default::default(),
            secrets: Default::default(),
            persistent_response: Default::default(),
            test_mode,
        })
    }

    #[test]
    fn serialize_config() {
        let readers = json!({
           "table_1": ["module_a"],
           "table_2": ["module_b"]
        });
        let readers = from_value::<HashMap<String, HashSet<String>>>(readers).unwrap();
        let writers = json!({
           "table_1": ["module_a"],
           "table_2": ["module_b"]
        });
        let writers = from_value::<HashMap<String, HashSet<String>>>(writers).unwrap();

        let cfg = DynamoDbConfig {
            authentication: AwsAuthentication::Iam {},
            local_endpoint: true,
            r: readers,
            rw: writers,
            reserved_tables: None,
        };

        println!("{}", toml::to_string(&cfg).unwrap());
    }

    #[tokio::test]
    async fn permission_checks() {
        let table_name = String::from("local_test");
        // permissions
        let readers = json!({table_name.clone(): ["module_a"]});
        let readers = from_value::<HashMap<String, HashSet<String>>>(readers).unwrap();

        let writers = json!({table_name.clone(): ["module_b"]});
        let writers = from_value::<HashMap<String, HashSet<String>>>(writers).unwrap();

        let client = DynamoDb::local_endpoint(readers, writers, None).await;

        // modules
        let module_a = test_module("module_a", true); // reader
        let module_b = test_module("module_b", true); // writer
        let module_c = test_module("module_c", true); // no access

        // modules can read table
        client
            .check_module_permissions(AccessScope::Read, module_a.clone(), &table_name)
            .unwrap();
        client
            .check_module_permissions(AccessScope::Read, module_b.clone(), &table_name)
            .unwrap();
        client
            .check_module_permissions(AccessScope::Read, module_c.clone(), &table_name)
            .expect_err("expect to fail with BadRequest");

        // readers can't write
        client
            .check_module_permissions(AccessScope::Write, module_a.clone(), &table_name)
            .expect_err("expect to fail with BadRequest");
        client
            .check_module_permissions(AccessScope::Write, module_b.clone(), &table_name)
            .unwrap();
        client
            .check_module_permissions(AccessScope::Write, module_c.clone(), &table_name)
            .expect_err("expect to fail with BadRequest");

        // unknown table
        client
            .check_module_permissions(AccessScope::Read, module_a.clone(), "unknown_table")
            .expect_err("expect to fail with BadRequest");
        client
            .check_module_permissions(AccessScope::Read, module_b.clone(), "unknown_table")
            .expect_err("expect to fail with BadRequest");
        client
            .check_module_permissions(AccessScope::Read, module_c.clone(), "unknown_table")
            .expect_err("expect to fail with BadRequest");

        // TODO: add test case for reserved table names
    }

    #[tokio::test]
    async fn put_query_delete() {
        // Initialize the client
        let table_name = String::from("local_test");
        let rw = json!({table_name.clone(): ["test_module"]});
        let rw = from_value::<HashMap<String, HashSet<String>>>(rw).unwrap();
        let cfg = DynamoDbConfig {
            local_endpoint: true,
            authentication: AwsAuthentication::Iam {},
            rw,
            r: HashMap::new(),
            reserved_tables: None,
        };
        let client = DynamoDb::new(cfg).await;
        let m = test_module("test_module", true);

        // PutItem with all attribute types
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
            condition_expression: None,
            expression_attribute_names: None,
            expression_attribute_values: None,
        };
        let input = serde_json::to_string(&input).unwrap();
        let output = client.put_item(&input, m.clone()).await.unwrap();
        let output_json = from_str::<Value>(&output).unwrap();
        assert_eq!(output_json, json!({"attributes":null}));

        // Query
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
            index_name: None,
        };
        let input = serde_json::to_string(&input).unwrap();
        let output = client.query(&input, m.clone()).await.unwrap();
        let output_json = from_str::<Value>(&output).unwrap();
        assert_eq!(
            output_json,
            json!({"items":[{"age":33,"binaries":["ZGF0YTE=","ZGF0YTI="],"binary_field":{"_binary":"YmluYXJ5X2RhdGE="},"is_active":true,"metadata":{"city":"New York","country":"USA"},"name":"Jane Doe","null_field":null,"pk":"124","ratings":[3.8,4.5,5],"scores":[88,92,95],"tags":["aws","dev","rust"],"timestamp":"124"}]})
        );

        // Delete Item
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
            condition_expression: None,
            expression_attribute_names: None,
            expression_attribute_values: None,
        };

        let input = serde_json::to_string(&input).unwrap();
        let output = client.delete_item(&input, m.clone()).await.unwrap();
        let output_json = from_str::<Value>(&output).unwrap();
        assert_eq!(
            output_json,
            json!({"attributes":{"age":33,"binaries":["ZGF0YTE=","ZGF0YTI="],"binary_field":{"_binary":"YmluYXJ5X2RhdGE="},"is_active":true,"metadata":{"city":"New York","country":"USA"},"name":"Jane Doe","null_field":null,"pk":"124","ratings":[3.8,4.5,5],"scores":[88,92,95],"tags":["aws","dev","rust"],"timestamp":"124"}})
        );
    }
}
