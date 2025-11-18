use crate::apis::ApiError;
use aws_sdk_dynamodb::primitives::Blob;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Converts DynamoDB Attributes into JSON Object
/// Convience method so the caller of the plaid DynamoDB can submit items in simple
/// JSON format and we will automatically convert into DynamoDB attributes which
/// is the internal representation of the Item data.
/// More Info
/// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/WorkingWithItems.html
/// https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_AttributeValue.html
pub fn attributes_into_json(attrs: &HashMap<String, AttributeValue>) -> Result<Value, ApiError> {
    let mut result = Map::new();
    for (k, v) in attrs.iter() {
        let new_val = to_json_value(v)?;
        result.insert(k.to_string(), new_val);
    }
    Ok(Value::Object(result))
}

/// Converts JSON Object into DynamoDB Attributes
/// Convience method so the caller of the plaid DynamoDB can submit items in simple
/// JSON format and we will automatically convert into DynamoDB attributes which
/// is the internal representation of the Item data.
/// More Info
/// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/WorkingWithItems.html
/// https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_AttributeValue.html
pub fn json_into_attributes(
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

/// converts String into ReturnValue for DynamoDB API
pub fn return_value_from_string(value: Option<String>) -> Result<Option<ReturnValue>, ApiError> {
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

/// categories of array members
/// used for conversion of json to strongly typed array
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

/// helper function to convert DynamoDB AttributeValue to JSON Value
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
