use std::collections::HashMap;

use base64::{prelude::BASE64_STANDARD, Engine};
use plaid_stl::{
    aws::dynamodb::{self, PutItemInput},
    entrypoint_with_source,
    messages::LogSource,
    network::make_named_request,
    plaid,
};

entrypoint_with_source!();

fn main(log: String, _: LogSource) -> Result<(), i32> {
    plaid::print_debug_string(&format!("Testing dynamodb With Log: [{log}]"));
    let table_name = String::from("local_test");
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
        Err(err) => plaid::print_debug_string(&format!("put_item error: {err}")),
        Ok(output) => {
            let json = serde_json::to_string(&output).unwrap();
            plaid::print_debug_string(&format!("put_item output: {json}"));
        }
    };

    Ok(())
}
