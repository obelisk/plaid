use std::{
    collections::HashMap,
    fmt::Display,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PlaidFunctionError;

/// Request sent to the runtime to query a BigQuery table.
#[derive(Deserialize, Serialize, Clone)]
pub struct QueryTableRequest {
    /// Dataset where `table` lives
    pub dataset: String,
    /// Name of the table to query
    pub table: String,
    /// Columns to select. Must be non-empty; the runtime does not support
    /// `SELECT *` so that callers are always explicit about what data they
    /// need and the runtime can return named results.
    pub columns: Vec<String>,
    /// Optional WHERE clause. When `None` the query returns all rows.
    pub filter: Option<Filter>,
}

/// A node in a WHERE clause expression tree.
///
/// Conditions can be nested arbitrarily using `And` and `Or`. The runtime
/// validates all column names and renders the tree into safe BigQuery SQL —
/// modules never construct raw SQL strings.
///
/// # Example
///
/// ```ignore
/// // WHERE (status = 'active' AND login_count > 5)
/// let filter = Filter::And(vec![
///     Filter::Condition {
///         column: "status".into(),
///         operator: Operator::Eq,
///         value: FilterValue::String("active".into()),
///     },
///     Filter::Condition {
///         column: "login_count".into(),
///         operator: Operator::Gt,
///         value: FilterValue::Integer(5),
///     },
/// ]);
/// ```
#[derive(Serialize, Deserialize, Clone)]
pub enum Filter {
    /// All child conditions must be true.
    And(Vec<Filter>),
    /// At least one child condition must be true.
    Or(Vec<Filter>),
    /// A single column comparison.
    Condition {
        column: String,
        operator: Operator,
        value: FilterValue,
    },
}

/// Comparison operator for a [`Filter::Condition`].
#[derive(Serialize, Deserialize, Clone)]
pub enum Operator {
    /// `=`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `LIKE` — use `%` and `_` wildcards in a [`FilterValue::String`].
    Like,
    /// `IS NULL` — no value is required; the runtime ignores the `value` field.
    IsNull,
    /// `IS NOT NULL` — no value is required; the runtime ignores the `value` field.
    IsNotNull,
}

/// The right-hand-side value for a [`Filter::Condition`].
#[derive(Serialize, Deserialize, Clone)]
pub enum FilterValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Null,
}

/// Response returned by the runtime for a BigQuery query.
///
/// Each row is a [`HashMap`] keyed by column name. NULL database values are
/// represented as [`Value::Null`].
///
/// `QueryTableResponse` implements [`Deref`] to `[HashMap<String, Value>]` and
/// both consuming and borrowing [`IntoIterator`], so it can be used directly as
/// a collection without accessing the inner field:
///
/// ```ignore
/// let rows = query_table("my_dataset", "events", &["user_id", "count"])?;
///
/// for row in &rows {
///     let user = &row["user_id"];   // Value::String
///     let count = &row["count"];    // Value::Number (if schema declares integer)
/// }
///
/// println!("{} rows returned", rows.len());
/// ```
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct QueryTableResponse {
    pub rows: Vec<HashMap<String, Value>>,
}

impl Deref for QueryTableResponse {
    type Target = [HashMap<String, Value>];
    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}

impl DerefMut for QueryTableResponse {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rows
    }
}

impl IntoIterator for QueryTableResponse {
    type Item = HashMap<String, Value>;
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.into_iter()
    }
}

impl<'a> IntoIterator for &'a QueryTableResponse {
    type Item = &'a HashMap<String, Value>;
    type IntoIter = std::slice::Iter<'a, HashMap<String, Value>>;
    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}

/// Query a BigQuery table.
///
/// `columns` must be non-empty. Specify exactly which columns you need;
/// the runtime will reject requests that do not name at least one column.
///
/// Returns a [`QueryTableResponse`] that can be iterated directly or indexed
/// like a slice. Each row is a [`HashMap`] keyed by the column names supplied
/// in `columns`. NULL database values are represented as [`Value::Null`].
///
/// Pass `filter` to add a WHERE clause. Use [`Filter`] to build the condition
/// tree — the runtime validates all identifiers and renders the SQL safely.
pub fn query_table(
    dataset: impl Display,
    table: impl Display,
    columns: &[impl Display],
    filter: Option<Filter>,
) -> Result<QueryTableResponse, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(gcp_bigquery, query_table);
    }

    let params = QueryTableRequest {
        dataset: dataset.to_string(),
        table: table.to_string(),
        columns: columns.iter().map(|c| c.to_string()).collect(),
        filter,
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
