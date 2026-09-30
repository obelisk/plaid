//! # PostgreSQL Reader Example
//!
//! Demonstrates a parameterized read from a Plaid rule through a named,
//! administrator-configured PostgreSQL connection. The webhook caller supplies
//! typed filters; the rule owns the SQL statement.

use plaid_stl::{
    entrypoint_with_source_and_response,
    messages::LogSource,
    plaid,
    postgres::{self, QueryParameter, QueryRequest},
};
use serde::{Deserialize, Serialize};

entrypoint_with_source_and_response!();

const CONNECTION: &str = "local_readonly";
const RETURN_BUFFER_SIZE: usize = 64 * 1024;
const QUERY: &str = "SELECT id, name, active, profile \
    FROM demo.people \
    WHERE id >= $1::bigint AND active = $2::boolean \
    ORDER BY id";

#[derive(Deserialize)]
struct Input {
    minimum_id: i64,
    active: bool,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn error_response(error: impl ToString) -> String {
    serde_json::to_string(&ErrorResponse {
        error: error.to_string(),
    })
    .expect("serializing an error string cannot fail")
}

fn main(data: String, _source: LogSource) -> Result<Option<String>, i32> {
    let input: Input = match serde_json::from_str(&data) {
        Ok(input) => input,
        Err(error) => return Ok(Some(error_response(format!("invalid JSON: {error}")))),
    };

    let request = QueryRequest {
        connection: CONNECTION.to_string(),
        sql: QUERY.to_string(),
        parameters: vec![
            QueryParameter::Integer(input.minimum_id),
            QueryParameter::Boolean(input.active),
        ],
    };

    let response = match postgres::query(request, RETURN_BUFFER_SIZE) {
        Ok(response) => response,
        Err(error) => {
            plaid::print_debug_string(&format!("[postgres-reader] query failed: {error}"));
            return Ok(Some(error_response(format!(
                "PostgreSQL query failed: {error}"
            ))));
        }
    };

    plaid::print_debug_string(&format!(
        "[postgres-reader] returned {} row(s)",
        response.rows.len()
    ));

    Ok(Some(
        serde_json::to_string(&response).expect("query responses must be serializable"),
    ))
}
