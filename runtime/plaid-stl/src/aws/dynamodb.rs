use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Default)]
pub struct PutItemInput {
    pub table_name: String,
    pub item: HashMap<String, Value>,
    pub expression_attribute_names: Option<HashMap<String, String>>,
    pub expression_attribute_values: Option<HashMap<String, Value>>,
    pub condition_expression: Option<String>,
    pub return_values: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct DeleteItemInput {
    pub table_name: String,
    pub key: HashMap<String, Value>,
    pub key_condition_expression: Option<String>,
    pub expression_attribute_names: Option<HashMap<String, String>>,
    pub expression_attribute_values: Option<HashMap<String, Value>>,
    pub return_values: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct QueryInput {
    pub table_name: String,
    pub index_name: Option<String>,
    pub key_condition_expression: String,
    pub expression_attribute_names: Option<HashMap<String, String>>,
    pub expression_attribute_values: Option<HashMap<String, Value>>,
}
