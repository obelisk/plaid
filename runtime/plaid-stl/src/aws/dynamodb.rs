use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 1024 * 1024 * 4; // 4 MiB

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
pub struct PutItemOutput {
    pub attributes: Option<Value>,
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
pub struct DeleteItemOutput {
    pub attributes: Option<Value>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct QueryInput {
    pub table_name: String,
    pub index_name: Option<String>,
    pub key_condition_expression: String,
    pub expression_attribute_names: Option<HashMap<String, String>>,
    pub expression_attribute_values: Option<HashMap<String, Value>>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct QueryOutput {
    pub items: Vec<Value>,
}

/// Put item in dynamodb table.
pub fn put_item(input: PutItemInput) -> Result<PutItemOutput, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_dynamodb, put_item);
    }

    let input = serde_json::to_string(&input).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        aws_dynamodb_put_item(
            input.as_ptr(),
            input.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    serde_json::from_slice::<PutItemOutput>(&return_buffer)
        .map_err(|_| PlaidFunctionError::InternalApiError)
}

/// Delete item in dynamodb table.
pub fn delete_item(input: DeleteItemInput) -> Result<DeleteItemOutput, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_dynamodb, delete_item);
    }

    let input = serde_json::to_string(&input).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        aws_dynamodb_delete_item(
            input.as_ptr(),
            input.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    serde_json::from_slice::<DeleteItemOutput>(&return_buffer)
        .map_err(|_| PlaidFunctionError::InternalApiError)
}

/// Query dynamodb table.
pub fn query(input: QueryInput) -> Result<QueryOutput, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_dynamodb, query);
    }

    let input = serde_json::to_string(&input).unwrap();

    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let res = unsafe {
        aws_dynamodb_query(
            input.as_ptr(),
            input.len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    // There was an error with the Plaid system. Maybe the API is not
    // configured.
    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    serde_json::from_slice::<QueryOutput>(&return_buffer)
        .map_err(|_| PlaidFunctionError::InternalApiError)
}
