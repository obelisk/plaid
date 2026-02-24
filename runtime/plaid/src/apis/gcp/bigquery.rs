use std::{collections::HashMap, sync::Arc};

use google_cloud_bigquery::{
    client::{google_cloud_auth, Client, ClientConfig},
    http::job::query::QueryRequest,
    query::row::Row,
};
use plaid_stl::gcp::bigquery::{
    Filter, FilterValue, Operator, ReadTableRequest, ReadTableResponse,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{apis::ApiError, loader::PlaidModule};
use log::error;

const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

#[derive(Error, Debug)]
pub enum BigQueryError {
    #[error("Authentication error: {0}")]
    Auth(#[from] google_cloud_auth::error::Error),
    #[error("Client initialization error: {0}")]
    Client(String),
    #[error("Query error: {0}")]
    QueryError(#[from] google_cloud_bigquery::client::QueryError),
    #[error("Row iteration error: {0}")]
    IterError(#[from] google_cloud_bigquery::query::Error),
    #[error("Row decode error: {0}")]
    RowError(#[from] google_cloud_bigquery::query::row::Error),
}

/// The BigQuery type a column should be decoded into.
///
/// The BigQuery HTTP API returns all values as strings on the wire, so without
/// explicit type information the runtime would always produce
/// `serde_json::Value::String`. Providing a `ColumnType` in the schema causes
/// the runtime to parse the raw string into the correct JSON primitive before
/// returning it to the module.
#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ColumnType {
    /// UTF-8 text. Produces `Value::String`. This is the default when no
    /// schema entry is present for a column.
    String,
    /// 64-bit signed integer. Produces `Value::Number`.
    Integer,
    /// 64-bit IEEE 754 float. Produces `Value::Number`.
    Float,
    /// Boolean. Produces `Value::Bool`.
    Boolean,
}

/// Mirrors the fields of a GCP service account JSON key file that are required
/// to authenticate as a service account. Credentials are supplied inline in the
/// Plaid config rather than via a key file on disk.
#[derive(Deserialize)]
pub struct BigQueryConfig {
    /// GCP project ID (e.g. "my-gcp-project")
    project_id: String,
    /// Service account email (e.g. "my-sa@my-project.iam.gserviceaccount.com")
    client_email: String,
    /// RSA private key in PEM format (the `private_key` field from the JSON key file)
    private_key: String,
    /// Private key ID (the `private_key_id` field from the JSON key file)
    private_key_id: String,
    /// Read access map: module name → dataset → list of tables the module may query
    r: HashMap<String, HashMap<String, Vec<String>>>,
    /// Column type schema: dataset → table → column name → [`ColumnType`].
    ///
    /// Columns not present in the schema are decoded as `String`. Providing a
    /// schema entry ensures numeric and boolean values arrive with the correct
    /// JSON type rather than being silently coerced to strings.
    #[serde(default)]
    schemas: HashMap<String, HashMap<String, HashMap<String, ColumnType>>>,
    /// Timeout (in milliseconds) applied to all queries
    #[serde(default = "default_timeout_ms")]
    timeout_ms: i64,
}

/// The default timeout if none is provided.
fn default_timeout_ms() -> i64 {
    50000
}

pub struct BigQuery {
    client: Client,
    project_id: String,
    config: BigQueryConfig,
}

impl BigQuery {
    pub async fn new(config: BigQueryConfig) -> Result<Self, BigQueryError> {
        // Build a CredentialsFile JSON from our config fields
        let credentials_json = serde_json::json!({
            "type": "service_account",
            "project_id": config.project_id,
            "client_email": config.client_email,
            "private_key_id": config.private_key_id,
            "private_key": config.private_key,
            "token_uri": TOKEN_URI,
        });

        let credentials = google_cloud_auth::credentials::CredentialsFile::new_from_str(
            &credentials_json.to_string(),
        )
        .await?;

        let (client_config, resolved_project_id) =
            ClientConfig::new_with_credentials(credentials).await?;

        // The project ID resolved from the credentials should match config.project_id,
        // but we fall back to config.project_id if the token source didn't return one.
        let project_id = resolved_project_id.unwrap_or_else(|| config.project_id.clone());

        let client = Client::new(client_config)
            .await
            .map_err(|e| BigQueryError::Client(e.to_string()))?;

        Ok(BigQuery {
            client,
            project_id,
            config,
        })
    }

    /// Executes a `SELECT` query against a BigQuery table and returns the
    /// results serialised as a [`ReadTableResponse`] JSON string.
    ///
    /// # Flow
    ///
    /// 1. Deserializes `params` into a [`ReadTableRequest`] (dataset, table,
    ///    columns). Returns `BadRequest` if the payload is malformed.
    /// 2. Calls [`build_query_request`][Self::build_query_request], which
    ///    enforces module permissions and validates all identifiers.
    /// 3. Issues the query via the BigQuery client and drains the async
    ///    iterator into a `Vec` of rows.
    /// 4. For each cell, looks up the column's [`ColumnType`] from the
    ///    configured schema (defaulting to `String` when absent) and calls
    ///    [`decode_column`] to produce the correct `serde_json::Value` variant.
    /// 5. Wraps the rows in a [`ReadTableResponse`] and serializes to JSON.
    ///
    /// # Errors
    ///
    /// Returns `ApiError::BadRequest` if permissions or identifier validation
    /// fails, and `ApiError::BigQueryError` for any network or decoding error.
    pub async fn query_table(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let params =
            serde_json::from_str::<ReadTableRequest>(params).map_err(|_| ApiError::BadRequest)?;

        let query_request = self.build_query_request(&module, &params)?;

        let mut iter = self
            .client
            .query::<Row>(&self.project_id, query_request)
            .await
            .map_err(BigQueryError::from)?;

        // Resolve the column type schema once for the requested table so we
        // don't repeat the map lookups inside the hot row-iteration loop.
        let table_schema = self
            .config
            .schemas
            .get(&params.dataset)
            .and_then(|d| d.get(&params.table));

        // Stream rows from BigQuery one at a time. For each row, walk the
        // requested columns in order — BigQuery returns them positionally, so
        // the column index `i` is the correct handle into the row. The schema
        // lookup drives decode_column so that integers, floats, and booleans
        // are preserved as their native JSON types rather than being coerced to
        // strings. Columns absent from the schema fall back to String, which is
        // always safe to attempt.
        let mut rows = Vec::new();
        while let Some(row) = iter.next().await.map_err(BigQueryError::from)? {
            let mut map = HashMap::new();
            for (i, col_name) in params.columns.iter().enumerate() {
                let col_type = table_schema
                    .and_then(|s| s.get(col_name))
                    .copied()
                    .unwrap_or(ColumnType::String);
                let value = decode_column(&row, i, col_type).map_err(BigQueryError::from)?;
                map.insert(col_name.clone(), value);
            }
            rows.push(map);
        }

        let response = ReadTableResponse { rows };
        serde_json::to_string(&response).map_err(|_| ApiError::ImpossibleError)
    }

    /// Returns `Ok(())` if the module has an entry in `r` for the given dataset
    /// that includes the requested table.
    fn check_module_permission(
        &self,
        module: &Arc<PlaidModule>,
        dataset: &str,
        table: &str,
    ) -> Result<(), ApiError> {
        let Some(datasets) = self.config.r.get(&module.to_string()) else {
            error!("[{module}] attempted to read BigQuery table [{dataset}.{table}] but has no BigQuery permissions configured");
            return Err(ApiError::BadRequest);
        };

        let Some(tables) = datasets.get(dataset) else {
            error!("[{module}] attempted to read BigQuery table [{dataset}.{table}] but is not permitted to access dataset [{dataset}]");
            return Err(ApiError::BadRequest);
        };

        if !tables.iter().any(|t| t == table) {
            error!("[{module}] attempted to read BigQuery table [{dataset}.{table}] but [{table}] is not in the permitted tables for dataset [{dataset}]");
            return Err(ApiError::BadRequest);
        }

        Ok(())
    }

    /// Validates permissions and constructs a [`QueryRequest`] ready to send to
    /// BigQuery.
    ///
    /// Combines the two validation steps that must both pass before a query is
    /// issued:
    ///
    /// 1. **Permission check** — asserts that `module` is allowed to read from
    ///    `params.dataset` and `params.table` according to the `r` config map.
    /// 2. **Query construction** — delegates to [`build_query_string`], which
    ///    validates all identifiers against a strict allowlist and assembles the
    ///    `SELECT` statement.
    ///
    /// Returns `ApiError::BadRequest` if either check fails.
    fn build_query_request(
        &self,
        module: &Arc<PlaidModule>,
        params: &ReadTableRequest,
    ) -> Result<QueryRequest, ApiError> {
        self.check_module_permission(module, &params.dataset, &params.table)?;
        let query = build_query_string(
            &params.dataset,
            &params.table,
            &params.columns,
            params.filter.as_ref(),
        )?;
        Ok(QueryRequest {
            timeout_ms: Some(self.config.timeout_ms),
            query,
            ..Default::default()
        })
    }
}

/// Validates every identifier and builds the full `SELECT … FROM … [WHERE …]`
/// query string.
///
/// Column names, dataset, and table are validated against a strict identifier
/// allowlist. The optional `filter` is rendered via [`build_filter_sql`], which
/// recursively validates all column names inside the condition tree.
fn build_query_string(
    dataset: &str,
    table: &str,
    columns: &[String],
    filter: Option<&Filter>,
) -> Result<String, ApiError> {
    if !is_valid_identifier(dataset) || !is_valid_identifier(table) {
        return Err(ApiError::BadRequest);
    }

    if columns.is_empty() {
        return Err(ApiError::BadRequest);
    }

    for col in columns {
        if !is_valid_identifier(col) {
            return Err(ApiError::BadRequest);
        }
    }

    let column_list = columns
        .iter()
        .map(|c| format!("`{c}`"))
        .collect::<Vec<_>>()
        .join(", ");

    let mut sql = format!("SELECT {column_list} FROM `{dataset}`.`{table}`");

    if let Some(f) = filter {
        sql.push_str(" WHERE ");
        sql.push_str(&build_filter_sql(f)?);
    }

    Ok(sql)
}

/// Recursively renders a [`Filter`] tree into a WHERE clause fragment.
///
/// Column names inside `Condition` nodes are validated with
/// [`is_valid_identifier`] before use. `And` and `Or` nodes must contain at
/// least one child.
fn build_filter_sql(filter: &Filter) -> Result<String, ApiError> {
    match filter {
        Filter::And(children) | Filter::Or(children) if children.is_empty() => {
            Err(ApiError::BadRequest)
        }
        Filter::And(children) => {
            let parts = children
                .iter()
                .map(build_filter_sql)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("({})", parts.join(" AND ")))
        }
        Filter::Or(children) => {
            let parts = children
                .iter()
                .map(build_filter_sql)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("({})", parts.join(" OR ")))
        }
        Filter::Condition {
            column,
            operator,
            value,
        } => {
            if !is_valid_identifier(column) {
                return Err(ApiError::BadRequest);
            }
            build_condition_sql(column, operator, value)
        }
    }
}

/// Renders a single `column OP value` condition.
///
/// `IsNull` and `IsNotNull` ignore `value` entirely.
fn build_condition_sql(
    column: &str,
    op: &Operator,
    value: &FilterValue,
) -> Result<String, ApiError> {
    let col = format!("`{column}`");
    match op {
        Operator::IsNull => return Ok(format!("{col} IS NULL")),
        Operator::IsNotNull => return Ok(format!("{col} IS NOT NULL")),
        _ => {}
    }

    let op_str = match op {
        Operator::Eq => "=",
        Operator::Ne => "<>",
        Operator::Lt => "<",
        Operator::Le => "<=",
        Operator::Gt => ">",
        Operator::Ge => ">=",
        Operator::Like => "LIKE",
        Operator::IsNull | Operator::IsNotNull => unreachable!(), // safety: returns above
    };

    Ok(format!("{col} {op_str} {}", build_value_sql(value)?))
}

/// Formats a [`FilterValue`] as a safe SQL literal.
///
/// Strings are wrapped in single quotes with internal single quotes doubled
/// (`'` → `''`), which is the standard SQL escaping mechanism. NaN and
/// infinite floats are rejected because BigQuery has no SQL representation for
/// them.
fn build_value_sql(value: &FilterValue) -> Result<String, ApiError> {
    match value {
        FilterValue::String(s) => {
            let escaped = s.replace('\'', "''");
            Ok(format!("'{escaped}'"))
        }
        FilterValue::Integer(n) => Ok(n.to_string()),
        FilterValue::Float(f) => {
            if f.is_nan() || f.is_infinite() {
                return Err(ApiError::BadRequest);
            }
            Ok(f.to_string())
        }
        FilterValue::Boolean(b) => Ok(if *b { "TRUE" } else { "FALSE" }.to_string()),
        FilterValue::Null => Ok("NULL".to_string()),
    }
}

/// Extracts the value at `index` from `row`, parsing it into the
/// `serde_json::Value` variant that corresponds to `col_type`.
///
/// The BigQuery HTTP API returns every value as a raw string on the wire.
/// Without schema information, `String` is the only safe choice. When a
/// `ColumnType` is provided the raw string is re-parsed into the correct
/// primitive so that modules receive `Value::Number` or `Value::Bool` instead
/// of a stringly-typed number that they would have to parse themselves.
fn decode_column(
    row: &Row,
    index: usize,
    col_type: ColumnType,
) -> Result<serde_json::Value, google_cloud_bigquery::query::row::Error> {
    Ok(match col_type {
        ColumnType::String => {
            let v: Option<String> = row.column(index)?;
            v.map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null)
        }
        ColumnType::Integer => {
            let v: Option<i64> = row.column(index)?;
            v.map(|n| serde_json::Value::Number(n.into()))
                .unwrap_or(serde_json::Value::Null)
        }
        ColumnType::Float => {
            let v: Option<f64> = row.column(index)?;
            v.and_then(serde_json::Number::from_f64)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        }
        ColumnType::Boolean => {
            let v: Option<bool> = row.column(index)?;
            v.map(serde_json::Value::Bool)
                .unwrap_or(serde_json::Value::Null)
        }
    })
}

/// Returns `true` if `s` is a valid BigQuery identifier: non-empty, starts
/// with an ASCII letter or underscore, and contains only ASCII letters, digits,
/// and underscores (`[A-Za-z_][A-Za-z0-9_]*`).
///
/// This is a strict allowlist. Any character outside that set — including
/// spaces, quotes, backticks, semicolons, parentheses, and comment markers
/// (`--`, `/*`) — causes the function to return `false`, making SQL injection
/// via crafted identifier names impossible regardless of the surrounding query
/// structure.
fn is_valid_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
