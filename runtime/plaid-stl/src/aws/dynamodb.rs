use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PlaidFunctionError;

const RETURN_BUFFER_SIZE: usize = 1024 * 1024 * 4; // 4 MiB

#[derive(Serialize, Deserialize)]
/// <p>Represents the input of a <code>PutItem</code> operation.</p>
pub struct PutItemInput {
    /// <p>The name of the table to contain the item. You can also provide the Amazon Resource Name (ARN) of the table in this parameter.</p>
    pub table_name: String,
    /// <p>A map of attribute name/value pairs, one for each attribute. Only the primary key attributes are required; you can optionally provide other attribute name-value pairs for the item.</p>
    /// <p>You must provide all of the attributes for the primary key. For example, with a simple primary key, you only need to provide a value for the partition key. For a composite primary key, you must provide both values for both the partition key and the sort key.</p>
    /// <p>If you specify any attributes that are part of an index key, then the data types for those attributes must match those of the schema in the table's attribute definition.</p>
    /// <p>Empty String and Binary attribute values are allowed. Attribute values of type String and Binary must have a length greater than zero if the attribute is used as a key attribute for a table or index.</p>
    /// <p>For more information about primary keys, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/HowItWorks.CoreComponents.html#HowItWorks.CoreComponents.PrimaryKey">Primary Key</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    /// <p>Each element in the <code>Item</code> map is an <code>AttributeValue</code> object.</p>
    pub item: HashMap<String, Value>,
    /// <p>One or more substitution tokens for attribute names in an expression. The following are some use cases for using <code>ExpressionAttributeNames</code>:</p>
    /// <ul>
    /// <li>
    /// <p>To access an attribute whose name conflicts with a DynamoDB reserved word.</p></li>
    /// <li>
    /// <p>To create a placeholder for repeating occurrences of an attribute name in an expression.</p></li>
    /// <li>
    /// <p>To prevent special characters in an attribute name from being misinterpreted in an expression.</p></li>
    /// </ul>
    /// <p>Use the <b>#</b> character in an expression to dereference an attribute name. For example, consider the following attribute name:</p>
    /// <ul>
    /// <li>
    /// <p><code>Percentile</code></p></li>
    /// </ul>
    /// <p>The name of this attribute conflicts with a reserved word, so it cannot be used directly in an expression. (For the complete list of reserved words, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/ReservedWords.html">Reserved Words</a> in the <i>Amazon DynamoDB Developer Guide</i>). To work around this, you could specify the following for <code>ExpressionAttributeNames</code>:</p>
    /// <ul>
    /// <li>
    /// <p><code>{"#P":"Percentile"}</code></p></li>
    /// </ul>
    /// <p>You could then use this substitution in an expression, as in this example:</p>
    /// <ul>
    /// <li>
    /// <p><code>#P = :val</code></p></li>
    /// </ul><note>
    /// <p>Tokens that begin with the <b>:</b> character are <i>expression attribute values</i>, which are placeholders for the actual value at runtime.</p>
    /// </note>
    /// <p>For more information on expression attribute names, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.AccessingItemAttributes.html">Specifying Item Attributes</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    pub expression_attribute_names: Option<HashMap<String, String>>,
    /// <p>One or more values that can be substituted in an expression.</p>
    /// <p>Use the <b>:</b> (colon) character in an expression to dereference an attribute value. For example, suppose that you wanted to check whether the value of the <i>ProductStatus</i> attribute was one of the following:</p>
    /// <p><code>Available | Backordered | Discontinued</code></p>
    /// <p>You would first need to specify <code>ExpressionAttributeValues</code> as follows:</p>
    /// <p><code>{ ":avail":{"S":"Available"}, ":back":{"S":"Backordered"}, ":disc":{"S":"Discontinued"} }</code></p>
    /// <p>You could then use these values in an expression, such as this:</p>
    /// <p><code>ProductStatus IN (:avail, :back, :disc)</code></p>
    /// <p>For more information on expression attribute values, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.SpecifyingConditions.html">Condition Expressions</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    pub expression_attribute_values: Option<HashMap<String, Value>>,
    /// <p>A condition that must be satisfied in order for a conditional <code>PutItem</code> operation to succeed.</p>
    /// <p>An expression can contain any of the following:</p>
    /// <ul>
    /// <li>
    /// <p>Functions: <code>attribute_exists | attribute_not_exists | attribute_type | contains | begins_with | size</code></p>
    /// <p>These function names are case-sensitive.</p></li>
    /// <li>
    /// <p>Comparison operators: <code>= | &lt;&gt; | &lt; | &gt; | &lt;= | &gt;= | BETWEEN | IN </code></p></li>
    /// <li>
    /// <p>Logical operators: <code>AND | OR | NOT</code></p></li>
    /// </ul>
    /// <p>For more information on condition expressions, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.SpecifyingConditions.html">Condition Expressions</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    pub condition_expression: Option<String>,
    /// <p>Use <code>ReturnValues</code> if you want to get the item attributes as they appeared before they were updated with the <code>PutItem</code> request. For <code>PutItem</code>, the valid values are:</p>
    /// <ul>
    /// <li>
    /// <p><code>NONE</code> - If <code>ReturnValues</code> is not specified, or if its value is <code>NONE</code>, then nothing is returned. (This setting is the default for <code>ReturnValues</code>.)</p></li>
    /// <li>
    /// <p><code>ALL_OLD</code> - If <code>PutItem</code> overwrote an attribute name-value pair, then the content of the old item is returned.</p></li>
    /// </ul>
    /// <p>The values returned are strongly consistent.</p>
    /// <p>There is no additional cost associated with requesting a return value aside from the small network and processing overhead of receiving a larger response. No read capacity units are consumed.</p><note>
    /// <p>The <code>ReturnValues</code> parameter is used by several DynamoDB operations; however, <code>PutItem</code> does not recognize any values other than <code>NONE</code> or <code>ALL_OLD</code>.</p>
    /// </note>
    pub return_values: Option<String>,
}

#[derive(Serialize, Deserialize)]
/// <p>Represents the output of a <code>PutItem</code> operation.</p>
pub struct PutItemOutput {
    /// <p>The attribute values as they appeared before the <code>PutItem</code> operation, but only if <code>ReturnValues</code> is specified as <code>ALL_OLD</code> in the request. Each element consists of an attribute name and an attribute value.</p>
    pub attributes: Option<Value>,
}

#[derive(Serialize, Deserialize)]
/// <p>Represents the input of a <code>DeleteItem</code> operation.</p>
pub struct DeleteItemInput {
    /// <p>The name of the table from which to delete the item. You can also provide the Amazon Resource Name (ARN) of the table in this parameter.</p>
    pub table_name: String,
    /// <p>A map of attribute names to <code>AttributeValue</code> objects, representing the primary key of the item to delete.</p>
    /// <p>For the primary key, you must provide all of the key attributes. For example, with a simple primary key, you only need to provide a value for the partition key. For a composite primary key, you must provide values for both the partition key and the sort key.</p>
    pub key: HashMap<String, Value>,
    /// <p>A condition that must be satisfied in order for a conditional <code>DeleteItem</code> to succeed.</p>
    /// <p>An expression can contain any of the following:</p>
    /// <ul>
    /// <li>
    /// <p>Functions: <code>attribute_exists | attribute_not_exists | attribute_type | contains | begins_with | size</code></p>
    /// <p>These function names are case-sensitive.</p></li>
    /// <li>
    /// <p>Comparison operators: <code>= | &lt;&gt; | &lt; | &gt; | &lt;= | &gt;= | BETWEEN | IN </code></p></li>
    /// <li>
    /// <p>Logical operators: <code>AND | OR | NOT</code></p></li>
    /// </ul>
    /// <p>For more information about condition expressions, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.SpecifyingConditions.html">Condition Expressions</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    pub condition_expression: Option<String>,
    /// <p>One or more substitution tokens for attribute names in an expression. The following are some use cases for using <code>ExpressionAttributeNames</code>:</p>
    /// <ul>
    /// <li>
    /// <p>To access an attribute whose name conflicts with a DynamoDB reserved word.</p></li>
    /// <li>
    /// <p>To create a placeholder for repeating occurrences of an attribute name in an expression.</p></li>
    /// <li>
    /// <p>To prevent special characters in an attribute name from being misinterpreted in an expression.</p></li>
    /// </ul>
    /// <p>Use the <b>#</b> character in an expression to dereference an attribute name. For example, consider the following attribute name:</p>
    /// <ul>
    /// <li>
    /// <p><code>Percentile</code></p></li>
    /// </ul>
    /// <p>The name of this attribute conflicts with a reserved word, so it cannot be used directly in an expression. (For the complete list of reserved words, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/ReservedWords.html">Reserved Words</a> in the <i>Amazon DynamoDB Developer Guide</i>). To work around this, you could specify the following for <code>ExpressionAttributeNames</code>:</p>
    /// <ul>
    /// <li>
    /// <p><code>{"#P":"Percentile"}</code></p></li>
    /// </ul>
    /// <p>You could then use this substitution in an expression, as in this example:</p>
    /// <ul>
    /// <li>
    /// <p><code>#P = :val</code></p></li>
    /// </ul><note>
    /// <p>Tokens that begin with the <b>:</b> character are <i>expression attribute values</i>, which are placeholders for the actual value at runtime.</p>
    /// </note>
    /// <p>For more information on expression attribute names, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.AccessingItemAttributes.html">Specifying Item Attributes</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    pub expression_attribute_names: Option<HashMap<String, String>>,
    /// <p>One or more values that can be substituted in an expression.</p>
    /// <p>Use the <b>:</b> (colon) character in an expression to dereference an attribute value. For example, suppose that you wanted to check whether the value of the <i>ProductStatus</i> attribute was one of the following:</p>
    /// <p><code>Available | Backordered | Discontinued</code></p>
    /// <p>You would first need to specify <code>ExpressionAttributeValues</code> as follows:</p>
    /// <p><code>{ ":avail":{"S":"Available"}, ":back":{"S":"Backordered"}, ":disc":{"S":"Discontinued"} }</code></p>
    /// <p>You could then use these values in an expression, such as this:</p>
    /// <p><code>ProductStatus IN (:avail, :back, :disc)</code></p>
    /// <p>For more information on expression attribute values, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.SpecifyingConditions.html">Condition Expressions</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    pub expression_attribute_values: Option<HashMap<String, Value>>,
    /// <p>Use <code>ReturnValues</code> if you want to get the item attributes as they appeared before they were deleted. For <code>DeleteItem</code>, the valid values are:</p>
    /// <ul>
    /// <li>
    /// <p><code>NONE</code> - If <code>ReturnValues</code> is not specified, or if its value is <code>NONE</code>, then nothing is returned. (This setting is the default for <code>ReturnValues</code>.)</p></li>
    /// <li>
    /// <p><code>ALL_OLD</code> - The content of the old item is returned.</p></li>
    /// </ul>
    /// <p>There is no additional cost associated with requesting a return value aside from the small network and processing overhead of receiving a larger response. No read capacity units are consumed.</p><note>
    /// <p>The <code>ReturnValues</code> parameter is used by several DynamoDB operations; however, <code>DeleteItem</code> does not recognize any values other than <code>NONE</code> or <code>ALL_OLD</code>.</p>
    /// </note>
    pub return_values: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct DeleteItemOutput {
    /// <p>A map of attribute names to <code>AttributeValue</code> objects, representing the item as it appeared before the <code>DeleteItem</code> operation. This map appears in the response only if <code>ReturnValues</code> was specified as <code>ALL_OLD</code> in the request.</p>
    pub attributes: Option<Value>,
}

#[derive(Serialize, Deserialize)]
pub struct QueryInput {
    /// <p>The name of the table containing the requested items. You can also provide the Amazon
    /// Resource Name (ARN) of the table in this parameter.</p>:W
    pub table_name: String,
    /// <p>The name of an index to query. This index can be any local secondary index or global secondary index on the table. Note that if you use the <code>IndexName</code> parameter, you must also provide <code>TableName.</code></p>
    pub index_name: Option<String>,
    /// <p>The condition that specifies the key values for items to be retrieved by the <code>Query</code> action.</p>
    /// <p>The condition must perform an equality test on a single partition key value.</p>
    /// <p>The condition can optionally perform one of several comparison tests on a single sort key value. This allows <code>Query</code> to retrieve one item with a given partition key value and sort key value, or several items that have the same partition key value but different sort key values.</p>
    /// <p>The partition key equality test is required, and must be specified in the following format:</p>
    /// <p><code>partitionKeyName</code> <i>=</i> <code>:partitionkeyval</code></p>
    /// <p>If you also want to provide a condition for the sort key, it must be combined using <code>AND</code> with the condition for the sort key. Following is an example, using the <b>=</b> comparison operator for the sort key:</p>
    /// <p><code>partitionKeyName</code> <code>=</code> <code>:partitionkeyval</code> <code>AND</code> <code>sortKeyName</code> <code>=</code> <code>:sortkeyval</code></p>
    /// <p>Valid comparisons for the sort key condition are as follows:</p>
    /// <ul>
    /// <li>
    /// <p><code>sortKeyName</code> <code>=</code> <code>:sortkeyval</code> - true if the sort key value is equal to <code>:sortkeyval</code>.</p></li>
    /// <li>
    /// <p><code>sortKeyName</code> <code>&lt;</code> <code>:sortkeyval</code> - true if the sort key value is less than <code>:sortkeyval</code>.</p></li>
    /// <li>
    /// <p><code>sortKeyName</code> <code>&lt;=</code> <code>:sortkeyval</code> - true if the sort key value is less than or equal to <code>:sortkeyval</code>.</p></li>
    /// <li>
    /// <p><code>sortKeyName</code> <code>&gt;</code> <code>:sortkeyval</code> - true if the sort key value is greater than <code>:sortkeyval</code>.</p></li>
    /// <li>
    /// <p><code>sortKeyName</code> <code>&gt;= </code> <code>:sortkeyval</code> - true if the sort key value is greater than or equal to <code>:sortkeyval</code>.</p></li>
    /// <li>
    /// <p><code>sortKeyName</code> <code>BETWEEN</code> <code>:sortkeyval1</code> <code>AND</code> <code>:sortkeyval2</code> - true if the sort key value is greater than or equal to <code>:sortkeyval1</code>, and less than or equal to <code>:sortkeyval2</code>.</p></li>
    /// <li>
    /// <p><code>begins_with (</code> <code>sortKeyName</code>, <code>:sortkeyval</code> <code>)</code> - true if the sort key value begins with a particular operand. (You cannot use this function with a sort key that is of type Number.) Note that the function name <code>begins_with</code> is case-sensitive.</p></li>
    /// </ul>
    /// <p>Use the <code>ExpressionAttributeValues</code> parameter to replace tokens such as <code>:partitionval</code> and <code>:sortval</code> with actual values at runtime.</p>
    /// <p>You can optionally use the <code>ExpressionAttributeNames</code> parameter to replace the names of the partition key and sort key with placeholder tokens. This option might be necessary if an attribute name conflicts with a DynamoDB reserved word. For example, the following <code>KeyConditionExpression</code> parameter causes an error because <i>Size</i> is a reserved word:</p>
    /// <ul>
    /// <li>
    /// <p><code>Size = :myval</code></p></li>
    /// </ul>
    /// <p>To work around this, define a placeholder (such a <code>#S</code>) to represent the attribute name <i>Size</i>. <code>KeyConditionExpression</code> then is as follows:</p>
    /// <ul>
    /// <li>
    /// <p><code>#S = :myval</code></p></li>
    /// </ul>
    /// <p>For a list of reserved words, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/ReservedWords.html">Reserved Words</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    /// <p>For more information on <code>ExpressionAttributeNames</code> and <code>ExpressionAttributeValues</code>, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/ExpressionPlaceholders.html">Using Placeholders for Attribute Names and Values</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    pub key_condition_expression: String,
    /// <p>One or more substitution tokens for attribute names in an expression. The following are some use cases for using <code>ExpressionAttributeNames</code>:</p>
    /// <ul>
    /// <li>
    /// <p>To access an attribute whose name conflicts with a DynamoDB reserved word.</p></li>
    /// <li>
    /// <p>To create a placeholder for repeating occurrences of an attribute name in an expression.</p></li>
    /// <li>
    /// <p>To prevent special characters in an attribute name from being misinterpreted in an expression.</p></li>
    /// </ul>
    /// <p>Use the <b>#</b> character in an expression to dereference an attribute name. For example, consider the following attribute name:</p>
    /// <ul>
    /// <li>
    /// <p><code>Percentile</code></p></li>
    /// </ul>
    /// <p>The name of this attribute conflicts with a reserved word, so it cannot be used directly in an expression. (For the complete list of reserved words, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/ReservedWords.html">Reserved Words</a> in the <i>Amazon DynamoDB Developer Guide</i>). To work around this, you could specify the following for <code>ExpressionAttributeNames</code>:</p>
    /// <ul>
    /// <li>
    /// <p><code>{"#P":"Percentile"}</code></p></li>
    /// </ul>
    /// <p>You could then use this substitution in an expression, as in this example:</p>
    /// <ul>
    /// <li>
    /// <p><code>#P = :val</code></p></li>
    /// </ul><note>
    /// <p>Tokens that begin with the <b>:</b> character are <i>expression attribute values</i>, which are placeholders for the actual value at runtime.</p>
    /// </note>
    /// <p>For more information on expression attribute names, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.AccessingItemAttributes.html">Specifying Item Attributes</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    pub expression_attribute_names: Option<HashMap<String, String>>,
    /// <p>One or more values that can be substituted in an expression.</p>
    /// <p>Use the <b>:</b> (colon) character in an expression to dereference an attribute value. For example, suppose that you wanted to check whether the value of the <i>ProductStatus</i> attribute was one of the following:</p>
    /// <p><code>Available | Backordered | Discontinued</code></p>
    /// <p>You would first need to specify <code>ExpressionAttributeValues</code> as follows:</p>
    /// <p><code>{ ":avail":{"S":"Available"}, ":back":{"S":"Backordered"}, ":disc":{"S":"Discontinued"} }</code></p>
    /// <p>You could then use these values in an expression, such as this:</p>
    /// <p><code>ProductStatus IN (:avail, :back, :disc)</code></p>
    /// <p>For more information on expression attribute values, see <a href="https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/Expressions.SpecifyingConditions.html">Specifying Conditions</a> in the <i>Amazon DynamoDB Developer Guide</i>.</p>
    pub expression_attribute_values: Option<HashMap<String, Value>>,
}

#[derive(Serialize, Deserialize)]
pub struct QueryOutput {
    pub items: Vec<Value>,
}

/// Put item in dynamodb table.
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

/// Delete item in dynamodb table.
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

/// Query dynamodb table.
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
