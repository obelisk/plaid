use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 1024 * 1024 * 4; // 4 MiB

#[derive(Serialize, Deserialize)]
/// Input for put_item operation
pub struct PutItemInput {
    /// The name of the table to contain the item. You can also provide the Amazon Resource Name (ARN) of the table in this parameter.
    pub table_name: String,
    /// A map of attribute name/value pairs, one for each attribute. Only the primary key attributes are required; you can optionally provide other attribute name-value pairs for the item.
    /// You must provide all of the attributes for the primary key. For example, with a simple primary key, you only need to provide a value for the partition key. For a composite primary key, you must provide both values for both the partition key and the sort key.
    ///
    /// More Info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/WorkingWithItems.html
    pub item: HashMap<String, Value>,
    /// One or more substitution tokens for attribute names in an expression. The following are some use cases for using ExpressionAttributeNames:
    /// * To access an attribute whose name conflicts with a DynamoDB reserved word.
    /// * To create a placeholder for repeating occurrences of an attribute name in an expression.
    /// * To prevent special characters in an attribute name from being misinterpreted in an expression.
    /// Use the # character in an expression to dereference an attribute name. For example, consider the following attribute name:
    ///
    ///     Percentile
    ///
    /// The name of this attribute conflicts with a reserved word, so it cannot be used directly in an expression. (For the complete list of reserved words, see Reserved Words in the Amazon DynamoDB Developer Guide). To work around this, you could specify the following for ExpressionAttributeNames:
    ///
    ///     {"#P":"Percentile"}
    ///
    /// You could then use this substitution in an expression, as in this example:
    ///
    ///     #P = :val
    ///
    /// More Info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.AccessingItemAttributes.html
    pub expression_attribute_names: Option<HashMap<String, String>>,
    /// One or more values that can be substituted in an expression.
    ///
    /// Use the : (colon) character in an expression to dereference an attribute value. For example, suppose that you wanted to check whether the value of the ProductStatus attribute was one of the following:
    ///
    ///     Available | Backordered | Discontinued
    ///
    /// You would first need to specify ExpressionAttributeValues as follows:
    ///
    ///     { ":avail":{"S":"Available"}, ":back":{"S":"Backordered"}, ":disc":{"S":"Discontinued"} }
    ///
    /// You could then use these values in an expression, such as this:
    ///
    ///     ProductStatus IN (:avail, :back, :disc)
    ///
    /// More info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.SpecifyingConditions.html
    pub expression_attribute_values: Option<HashMap<String, Value>>,
    /// A condition that must be satisfied in order for a conditional PutItem operation to succeed.
    ///
    /// An expression can contain any of the following:
    ///
    /// Functions: attribute_exists | attribute_not_exists | attribute_type | contains | begins_with | size
    ///
    /// These function names are case-sensitive.
    ///
    /// Comparison operators: = | <> | < | > | <= | >= | BETWEEN | IN
    ///
    /// Logical operators: AND | OR | NOT
    ///
    /// More Info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.SpecifyingConditions.html
    pub condition_expression: Option<String>,
    /// Use ReturnValues if you want to get the item attributes as they appeared before they were updated with the PutItem request. For PutItem, the valid values are:
    /// NONE - If ReturnValues is not specified, or if its value is NONE, then nothing is returned. (This setting is the default for ReturnValues.)
    /// ALL_OLD - If PutItem overwrote an attribute name-value pair, then the content of the old item is returned.
    /// The values returned are strongly consistent.
    pub return_values: Option<String>,
}

#[derive(Serialize, Deserialize)]
/// Output for put_item operation
pub struct PutItemOutput {
    /// The attribute values as they appeared before the PutItem operation,
    /// but only if ReturnValues is specified as ALL_OLD in the request.
    /// Each element consists of an attribute name and an attribute value.
    pub attributes: Option<Value>,
}

#[derive(Serialize, Deserialize)]
/// Input for delete_item operation
pub struct DeleteItemInput {
    /// The name of the table to contain the item. You can also provide the Amazon Resource Name (ARN) of the table in this parameter.
    pub table_name: String,
    /// A map of attribute names to AttributeValue objects, representing the primary key of the item to delete.
    /// For the primary key, you must provide all of the key attributes. For example, with a simple primary key, you only need to provide a value for the partition key. For a composite primary key, you must provide values for both the partition key and the sort key.
    pub key: HashMap<String, Value>,
    /// One or more substitution tokens for attribute names in an expression. The following are some use cases for using ExpressionAttributeNames:
    /// * To access an attribute whose name conflicts with a DynamoDB reserved word.
    /// * To create a placeholder for repeating occurrences of an attribute name in an expression.
    /// * To prevent special characters in an attribute name from being misinterpreted in an expression.
    /// Use the # character in an expression to dereference an attribute name. For example, consider the following attribute name:
    ///
    ///     Percentile
    ///
    /// The name of this attribute conflicts with a reserved word, so it cannot be used directly in an expression. (For the complete list of reserved words, see Reserved Words in the Amazon DynamoDB Developer Guide). To work around this, you could specify the following for ExpressionAttributeNames:
    ///
    ///     {"#P":"Percentile"}
    ///
    /// You could then use this substitution in an expression, as in this example:
    ///
    ///     #P = :val
    ///
    /// More Info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.AccessingItemAttributes.html
    pub expression_attribute_names: Option<HashMap<String, String>>,
    /// One or more values that can be substituted in an expression.
    ///
    /// Use the : (colon) character in an expression to dereference an attribute value. For example, suppose that you wanted to check whether the value of the ProductStatus attribute was one of the following:
    ///
    ///     Available | Backordered | Discontinued
    ///
    /// You would first need to specify ExpressionAttributeValues as follows:
    ///
    ///     { ":avail":{"S":"Available"}, ":back":{"S":"Backordered"}, ":disc":{"S":"Discontinued"} }
    ///
    /// You could then use these values in an expression, such as this:
    ///
    ///     ProductStatus IN (:avail, :back, :disc)
    ///
    /// More info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.SpecifyingConditions.html
    pub expression_attribute_values: Option<HashMap<String, Value>>,
    /// A condition that must be satisfied in order for a conditional PutItem operation to succeed.
    ///
    /// An expression can contain any of the following:
    ///
    /// Functions: attribute_exists | attribute_not_exists | attribute_type | contains | begins_with | size
    ///
    /// These function names are case-sensitive.
    ///
    /// Comparison operators: = | <> | < | > | <= | >= | BETWEEN | IN
    ///
    /// Logical operators: AND | OR | NOT
    ///
    /// More Info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.SpecifyingConditions.html
    pub condition_expression: Option<String>,
    /// Use ReturnValues if you want to get the item attributes as they appeared before they were updated with the PutItem request. For PutItem, the valid values are:
    /// NONE - If ReturnValues is not specified, or if its value is NONE, then nothing is returned. (This setting is the default for ReturnValues.)
    /// ALL_OLD - If PutItem overwrote an attribute name-value pair, then the content of the old item is returned.
    /// The values returned are strongly consistent.
    pub return_values: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct DeleteItemOutput {
    /// The attribute values as they appeared before the PutItem operation,
    /// but only if ReturnValues is specified as ALL_OLD in the request.
    /// Each element consists of an attribute name and an attribute value.
    pub attributes: Option<Value>,
}

#[derive(Serialize, Deserialize)]
/// Input for query operation
pub struct QueryInput {
    /// The name of the table to contain the item. You can also provide the Amazon Resource Name (ARN) of the table in this parameter.
    pub table_name: String,
    /// The name of an index to query. This index can be any local secondary index or global secondary index on the table.
    /// Note that if you use the IndexName parameter, you must also provide TableName.
    pub index_name: Option<String>,
    /// The condition that specifies the key values for items to be retrieved by the Query action.
    /// The condition must perform an equality test on a single partition key value.
    /// The condition can optionally perform one of several comparison tests on a single sort key value. This allows Query to retrieve one item with a given partition key value and sort key value, or several items that have the same partition key value but different sort key values.
    /// The partition key equality test is required, and must be specified in the following format:
    ///
    /// partitionKeyName = :partitionkeyval
    ///
    /// If you also want to provide a condition for the sort key, it must be combined using AND with the condition for the sort key. Following is an example, using the = comparison operator for the sort key:
    ///
    /// partitionKeyName = :partitionkeyval AND sortKeyName = :sortkeyval
    ///
    /// More Info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_Query.html#DDB-Query-request-KeyConditionExpression
    pub key_condition_expression: String,
    /// One or more substitution tokens for attribute names in an expression. The following are some use cases for using ExpressionAttributeNames:
    /// * To access an attribute whose name conflicts with a DynamoDB reserved word.
    /// * To create a placeholder for repeating occurrences of an attribute name in an expression.
    /// * To prevent special characters in an attribute name from being misinterpreted in an expression.
    /// Use the # character in an expression to dereference an attribute name. For example, consider the following attribute name:
    ///
    ///     Percentile
    ///
    /// The name of this attribute conflicts with a reserved word, so it cannot be used directly in an expression. (For the complete list of reserved words, see Reserved Words in the Amazon DynamoDB Developer Guide). To work around this, you could specify the following for ExpressionAttributeNames:
    ///
    ///     {"#P":"Percentile"}
    ///
    /// You could then use this substitution in an expression, as in this example:
    ///
    ///     #P = :val
    ///
    /// More Info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.AccessingItemAttributes.html
    pub expression_attribute_names: Option<HashMap<String, String>>,
    /// One or more values that can be substituted in an expression.
    ///
    /// Use the : (colon) character in an expression to dereference an attribute value. For example, suppose that you wanted to check whether the value of the ProductStatus attribute was one of the following:
    ///
    ///     Available | Backordered | Discontinued
    ///
    /// You would first need to specify ExpressionAttributeValues as follows:
    ///
    ///     { ":avail":{"S":"Available"}, ":back":{"S":"Backordered"}, ":disc":{"S":"Discontinued"} }
    ///
    /// You could then use these values in an expression, such as this:
    ///
    ///     ProductStatus IN (:avail, :back, :disc)
    ///
    /// More info
    /// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.SpecifyingConditions.html
    pub expression_attribute_values: Option<HashMap<String, Value>>,
}

#[derive(Serialize, Deserialize)]
pub struct QueryOutput {
    pub items: Vec<Value>,
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
pub fn put_item(input: PutItemInput) -> Result<PutItemOutput, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_dynamodb, put_item);
    }

    let input = serde_json::to_string(&input).map_err(|_| PlaidFunctionError::InternalApiError)?;

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

/// Deletes a single item in a table by primary key. You can perform a conditional delete operation that deletes the item if it exists, or if it has an expected attribute value.
/// In addition to deleting an item, you can also return the item's attribute values in the same operation, using the ReturnValues parameter.
/// Unless you specify conditions, the DeleteItem is an idempotent operation; running it multiple times on the same item or attribute does not result in an error response.
/// Conditional deletes are useful for deleting items only if specific conditions are met. If those conditions are met, DynamoDB performs the delete. Otherwise, the item is not deleted.
///
/// More Info
/// https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_DeleteItem.html
pub fn delete_item(input: DeleteItemInput) -> Result<DeleteItemOutput, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_dynamodb, delete_item);
    }

    let input = serde_json::to_string(&input).map_err(|_| PlaidFunctionError::InternalApiError)?;

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

/// You must provide the name of the partition key attribute and a single value for that attribute.
/// Query returns all items with that partition key value.
/// Optionally, you can provide a sort key attribute and use a comparison operator to refine the search results.
///
/// Use the KeyConditionExpression parameter to provide a specific value for the partition key.
/// The Query operation will return all of the items from the table or index with that partition key value.
///
/// More Info
/// https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_Query.html
pub fn query(input: QueryInput) -> Result<QueryOutput, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(aws_dynamodb, query);
    }

    let input = serde_json::to_string(&input).map_err(|_| PlaidFunctionError::InternalApiError)?;

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
