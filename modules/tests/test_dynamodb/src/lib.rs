use serde_json::{json, to_value, Value};
use std::collections::HashMap;

use base64::{prelude::BASE64_STANDARD, Engine};
use plaid_stl::{
    aws::dynamodb::{self, DeleteItemInput, PutItemInput, QueryInput},
    entrypoint_with_source,
    messages::LogSource,
    plaid,
};

entrypoint_with_source!();

fn main(log: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Testing dynamodb With Log: [{log}]"));
    let table_name = String::from("local_test");
    // PutItem (with all attribute types)
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
            BASE64_STANDARD.encode("data1"),
            BASE64_STANDARD.encode("data2")
        ],
        "binary_field": { "_binary": BASE64_STANDARD.encode("binary_data") } // Binary
    });
    let item_hm = serde_json::from_value::<HashMap<String, serde_json::Value>>(item_json).unwrap();
    let input = PutItemInput {
        table_name: table_name.clone(),
        item: item_hm,
        return_values: Some(String::from("ALL_OLD")),
        condition_expression: None,
        expression_attribute_names: None,
        expression_attribute_values: None,
    };

    match dynamodb::put_item(input) {
        Err(err) => {
            plaid::print_debug_string(&format!("put_item error: {err}"));
            return Err(1); // TODO: proper error code
        }
        Ok(output) => {
            let json = serde_json::to_string(&output).unwrap();
            plaid::print_debug_string(&format!("put_item output: {json}"));
            if json != json!({"attributes":null}) {
                plaid::print_debug_string(&format!("error: put_item output_json mismatch: {json}"));
                return Err(1); // TODO: proper error code
            }
        }
    };

    // Query
    let input = QueryInput {
        table_name: table_name.clone(),
        key_condition_expression: String::from("#pk = :val"),
        expression_attribute_names: Some(HashMap::from([("#pk".to_string(), "pk".to_string())])),
        expression_attribute_values: Some(HashMap::from([(
            ":val".to_string(),
            Value::String(String::from("124")),
        )])),
        index_name: None,
    };
    let output = dynamodb::query(input).unwrap();
    let output_json = to_value(&output).unwrap();
    if output_json
        != json!({"items":[{"age":33,"binaries":["ZGF0YTE=","ZGF0YTI="],"binary_field":{"_binary":"YmluYXJ5X2RhdGE="},"is_active":true,"metadata":{"city":"New York","country":"USA"},"name":"Jane Doe","null_field":null,"pk":"124","ratings":[3.8,4.5,5],"scores":[88,92,95],"tags":["aws","dev","rust"],"timestamp":"124"}]})
    {
        plaid::print_debug_string(&format!("error: query output_json mismatch: {output_json}"));
        return Err(1); // TODO: proper error code
    }

    // Delete Item
    let input = DeleteItemInput {
        table_name,
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

    let output = dynamodb::delete_item(input).unwrap();
    let output_json = to_value(&output).unwrap();
    if output_json
        != json!({"attributes":{"age":33,"binaries":["ZGF0YTE=","ZGF0YTI="],"binary_field":{"_binary":"YmluYXJ5X2RhdGE="},"is_active":true,"metadata":{"city":"New York","country":"USA"},"name":"Jane Doe","null_field":null,"pk":"124","ratings":[3.8,4.5,5],"scores":[88,92,95],"tags":["aws","dev","rust"],"timestamp":"124"}})
    {
        plaid::print_debug_string(&format!(
            "error: delete_item output_json mismatch: {output_json}"
        ));
        return Err(1); // TODO: proper error code
    }

    Ok(())
}
