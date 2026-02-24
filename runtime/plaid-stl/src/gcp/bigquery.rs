use std::{
    collections::HashMap,
    fmt::Display,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PlaidFunctionError;

/// Request sent to the runtime to read rows from a BigQuery table.
#[derive(Deserialize, Serialize)]
pub struct ReadTableRequest {
    pub dataset: String,
    pub table: String,
    /// Columns to select. Must be non-empty; the runtime does not support
    /// `SELECT *` so that callers are always explicit about what data they
    /// need and the runtime can return named results.
    pub columns: Vec<String>,
}

/// Response returned by the runtime for a BigQuery read.
///
/// Each row is a [`HashMap`] keyed by column name. NULL database values are
/// represented as [`Value::Null`].
///
/// `ReadTableResponse` implements [`Deref`] to `[HashMap<String, Value>]` and
/// both consuming and borrowing [`IntoIterator`], so it can be used directly as
/// a collection without accessing the inner field:
///
/// ```ignore
/// let rows = read_from_table("my_dataset", "events", &["user_id", "count"])?;
///
/// for row in &rows {
///     let user = &row["user_id"];   // Value::String
///     let count = &row["count"];    // Value::Number (if schema declares integer)
/// }
///
/// println!("{} rows returned", rows.len());
/// ```
#[derive(Deserialize, Serialize, Debug)]
pub struct ReadTableResponse {
    pub rows: Vec<HashMap<String, Value>>,
}

impl Deref for ReadTableResponse {
    type Target = [HashMap<String, Value>];
    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}

impl DerefMut for ReadTableResponse {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rows
    }
}

impl IntoIterator for ReadTableResponse {
    type Item = HashMap<String, Value>;
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.into_iter()
    }
}

impl<'a> IntoIterator for &'a ReadTableResponse {
    type Item = &'a HashMap<String, Value>;
    type IntoIter = std::slice::Iter<'a, HashMap<String, Value>>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}

/// Read rows from a BigQuery table.
///
/// `columns` must be non-empty. Specify exactly which columns you need;
/// the runtime will reject requests that do not name at least one column.
///
/// Returns a [`ReadTableResponse`] that can be iterated directly or indexed
/// like a slice. Each row is a [`HashMap`] keyed by the column names supplied
/// in `columns`. NULL database values are represented as [`Value::Null`].
pub fn query_table(
    dataset: impl Display,
    table: impl Display,
    columns: &[impl Display],
) -> Result<ReadTableResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(gcp_bigquery, query_table);
    }

    let params = ReadTableRequest {
        dataset: dataset.to_string(),
        table: table.to_string(),
        columns: columns.iter().map(|c| c.to_string()).collect(),
    };

    const RETURN_BUFFER_SIZE: usize = 1024 * 1024; // 1 MiB
    let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

    let params = serde_json::to_string(&params).unwrap();
    let res = unsafe {
        gcp_bigquery_query_table(
            params.as_bytes().as_ptr(),
            params.as_bytes().len(),
            return_buffer.as_mut_ptr(),
            RETURN_BUFFER_SIZE,
        )
    };

    if res < 0 {
        return Err(res.into());
    }

    return_buffer.truncate(res as usize);

    let response = String::from_utf8(return_buffer).unwrap();

    serde_json::from_str(&response).map_err(|_| PlaidFunctionError::InternalApiError)
}
