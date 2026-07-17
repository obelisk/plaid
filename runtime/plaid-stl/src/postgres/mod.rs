//! PostgreSQL query types and host-function wrapper.
//!
//! Connections and permissions are configured by the Plaid runtime. Rules only
//! receive a connection name and never receive database credentials.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PlaidFunctionError;

/// A value bound to a PostgreSQL `$n` query parameter.
///
/// The MVP intentionally supports only common scalar types. Queries can cast
/// values explicitly when PostgreSQL cannot infer a parameter type.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum QueryParameter {
    Null,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Json(Value),
}

/// Request sent to the runtime to execute one read query.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct QueryRequest {
    /// Name of a connection configured in Plaid.
    pub connection: String,
    /// One PostgreSQL statement. Use `$1`, `$2`, ... for parameters.
    pub sql: String,
    #[serde(default)]
    pub parameters: Vec<QueryParameter>,
}

/// Metadata for one returned column.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct QueryColumn {
    pub name: String,
    pub postgres_type: String,
}

/// Rows returned by a PostgreSQL query.
///
/// Row values are positional and correspond to `columns`. This preserves
/// duplicate column names produced by arbitrary SQL.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct QueryResponse {
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<Vec<Value>>,
}

/// Execute a parameterized read query through a configured PostgreSQL
/// connection.
///
/// `return_buffer_size` is guest memory reserved for the JSON response. The
/// runtime also applies its own configured response-size limit.
pub fn query(
    request: QueryRequest,
    return_buffer_size: usize,
) -> Result<QueryResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(postgres, query);
    }

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::ErrorCouldNotSerialize)?;
    let mut return_buffer = vec![0; return_buffer_size];

    let result = unsafe {
        postgres_query(
            request.as_ptr(),
            request.len(),
            return_buffer.as_mut_ptr(),
            return_buffer_size,
        )
    };

    if result < 0 {
        return Err(result.into());
    }

    return_buffer.truncate(result as usize);
    serde_json::from_slice(&return_buffer).map_err(|_| PlaidFunctionError::InternalApiError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_response_round_trip() {
        let request = QueryRequest {
            connection: "local".to_string(),
            sql: "SELECT $1::bigint, $2::jsonb".to_string(),
            parameters: vec![
                QueryParameter::Integer(42),
                QueryParameter::Json(serde_json::json!({"ok": true})),
            ],
        };

        let encoded = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<QueryRequest>(&encoded).unwrap(),
            request
        );

        let response = QueryResponse {
            columns: vec![QueryColumn {
                name: "value".to_string(),
                postgres_type: "int8".to_string(),
            }],
            rows: vec![vec![Value::from(42)]],
        };
        let encoded = serde_json::to_string(&response).unwrap();
        assert_eq!(
            serde_json::from_str::<QueryResponse>(&encoded).unwrap(),
            response
        );
    }
}
