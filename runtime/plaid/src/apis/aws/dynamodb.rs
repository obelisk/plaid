use aws_sdk_dynamodb::Client;
use plaid_stl::aws::dynamodb::{
    DeleteItemInput, DeleteItemOutput, PutItemInput, PutItemOutput, QueryInput, QueryOutput,
};

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::apis::{
    aws::dynamodb_utils::{attributes_into_json, json_into_attributes, return_value_from_string},
    AccessScope, ApiError,
};
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
    #[serde(default)]
    reserved_tables: Option<HashSet<String>>,
}

/// Represents the DynamoDB API client.
/// NOTE: if Plaid is configured with the DynamoDB database backend, sharing tables here will lead to undefined behaviour
pub struct DynamoDb {
    /// The underlying client used to interact with the Dynamodb API.
    client: Client,
    /// Configured writers - maps a table name to a list of rules that are allowed to READ or WRITE data
    rw: HashMap<String, HashSet<String>>,
    /// Configured readers - maps a table name to a list of rules that are allowed to READ data
    r: HashMap<String, HashSet<String>>,
    /// Reserved tables - list of 'reserved' table names which rules cannot access
    reserved_tables: Option<HashSet<String>>,
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

    /// Checks if a module can perform a given action
    /// Modules are registered as as read (R) or write (RW) under self.
    /// This function checks:
    /// * If the table is a reserved table i.e. no Module is allowed to access reserved tables.
    /// * If the module is configured as a Reader or Writer of a given table
    fn check_module_permissions(
        &self,
        access_scope: AccessScope,
        module: Arc<PlaidModule>,
        table_name: &str,
    ) -> Result<(), ApiError> {
        // Check if table is reserved table
        // no rule is allowed to operate on reserved table
        if let Some(inner) = &self.reserved_tables {
            if inner.contains(table_name) {
                warn!("[{module}] failed {access_scope:?} access reserved dynamodb table [{table_name}]");
                return Err(ApiError::BadRequest);
            }
        }

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

                // check if read access is configured for this table
                if let Some(table_readers) = self.r.get(table_name) {
                    // check if this module has read access to this table
                    if table_readers.contains(&module.to_string()) {
                        warn!(
                            "[{module}] trying to [write] but only has [read] permission for dynamodb table [{table_name}]"
                        );
                        return Err(ApiError::BadRequest);
                    }
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

#[cfg(test)]
pub mod tests {
    use serde_json::{from_str, from_value, json, Value};
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
        let reserved: HashSet<String> = vec![String::from("reserved")].into_iter().collect();
        let readers = json!({table_name.clone(): ["module_a"]});
        let readers = from_value::<HashMap<String, HashSet<String>>>(readers).unwrap();

        let writers = json!({table_name.clone(): ["module_b"]});
        let writers = from_value::<HashMap<String, HashSet<String>>>(writers).unwrap();

        let client = DynamoDb::local_endpoint(readers, writers, Some(reserved)).await;

        // modules
        let module_a = test_module("module_a", true); // reader
        let module_b = test_module("module_b", true); // writer
        let module_c = test_module("module_c", true); // no access

        // try access reserved table
        client
            .check_module_permissions(AccessScope::Read, module_a.clone(), "reserved")
            .expect_err("expect to fail with BadRequest");
        client
            .check_module_permissions(AccessScope::Write, module_a.clone(), "reserved")
            .expect_err("expect to fail with BadRequest");

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
            json!({
               "items":[
                  {
                     "age":33,
                     "binaries":[
                        "ZGF0YTE=",
                        "ZGF0YTI="
                     ],
                     "binary_field":{
                        "_binary":"YmluYXJ5X2RhdGE="
                     },
                     "is_active":true,
                     "metadata":{
                        "city":"New York",
                        "country":"USA"
                     },
                     "name":"Jane Doe",
                     "null_field":null,
                     "pk":"124",
                     "ratings":[
                        3.8,
                        4.5,
                        5
                     ],
                     "scores":[
                        88,
                        92,
                        95
                     ],
                     "tags":[
                        "aws",
                        "dev",
                        "rust"
                     ],
                     "timestamp":"124"
                  }
               ]
            })
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
            json!({
               "attributes":{
                  "age":33,
                  "binaries":[
                     "ZGF0YTE=",
                     "ZGF0YTI="
                  ],
                  "binary_field":{
                     "_binary":"YmluYXJ5X2RhdGE="
                  },
                  "is_active":true,
                  "metadata":{
                     "city":"New York",
                     "country":"USA"
                  },
                  "name":"Jane Doe",
                  "null_field":null,
                  "pk":"124",
                  "ratings":[
                     3.8,
                     4.5,
                     5
                  ],
                  "scores":[
                     88,
                     92,
                     95
                  ],
                  "tags":[
                     "aws",
                     "dev",
                     "rust"
                  ],
                  "timestamp":"124"
               }
            })
        );
    }
}
