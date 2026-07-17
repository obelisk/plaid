use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    num::{NonZeroU64, NonZeroUsize},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use futures_util::{pin_mut, TryStreamExt};
use plaid_stl::postgres::{QueryColumn, QueryParameter, QueryRequest, QueryResponse};
use serde::{de::Error as _, Deserialize, Deserializer};
use serde_json::Value;
use thiserror::Error;
use tokio_postgres::{
    types::{to_sql_checked, IsNull, Json, ToSql, Type},
    NoTls, Row,
};
use tokio_util::bytes::BytesMut;

use crate::{apis::ApiError, loader::PlaidModule};

const CLEAN_CONNECTION: &str = "CLOSE ALL;\
    SET SESSION AUTHORIZATION DEFAULT;\
    RESET ALL;\
    UNLISTEN *;\
    SELECT pg_advisory_unlock_all();\
    DISCARD TEMP;\
    DISCARD SEQUENCES;";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ConnectionName(String);

impl ConnectionName {
    fn new(name: String) -> Result<Self, String> {
        if name.is_empty() {
            return Err("PostgreSQL connection name cannot be empty".to_string());
        }

        Ok(Self(name))
    }
}

impl<'de> Deserialize<'de> for ConnectionName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl Borrow<str> for ConnectionName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ConnectionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

fn deserialize_connection_string<'de, D>(
    deserializer: D,
) -> Result<tokio_postgres::Config, D::Error>
where
    D: Deserializer<'de>,
{
    let connection_string = String::deserialize(deserializer)?;
    tokio_postgres::Config::from_str(&connection_string).map_err(D::Error::custom)
}

/// Configuration for all named PostgreSQL connections.
#[derive(Deserialize)]
pub struct PostgresConfig {
    connections: HashMap<ConnectionName, PostgresConnectionConfig>,
}

/// Configuration and safety limits for one PostgreSQL connection.
///
/// The MVP uses `NoTls`; TLS connection support can be added without changing
/// the rule-facing API. Credentials should be interpolated from Plaid secrets.
#[derive(Deserialize)]
struct PostgresConnectionConfig {
    #[serde(
        rename = "connection_string",
        deserialize_with = "deserialize_connection_string"
    )]
    postgres_config: tokio_postgres::Config,
    allowed_rules: HashSet<String>,
    #[serde(default = "default_max_pool_size")]
    max_pool_size: NonZeroUsize,
    #[serde(default = "default_pool_timeout_ms")]
    pool_timeout_ms: NonZeroU64,
    #[serde(default = "default_statement_timeout_ms")]
    statement_timeout_ms: NonZeroU64,
    #[serde(default = "default_lock_timeout_ms")]
    lock_timeout_ms: NonZeroU64,
    #[serde(default = "default_max_query_size")]
    max_query_size: NonZeroUsize,
    #[serde(default = "default_max_parameters")]
    max_parameters: NonZeroUsize,
    #[serde(default = "default_max_rows")]
    max_rows: NonZeroUsize,
    #[serde(default = "default_max_response_size")]
    max_response_size: NonZeroUsize,
}

struct PostgresConnection {
    pool: Pool,
    allowed_rules: HashSet<String>,
    pool_timeout_ms: NonZeroU64,
    statement_timeout_ms: NonZeroU64,
    lock_timeout_ms: NonZeroU64,
    max_query_size: NonZeroUsize,
    max_parameters: NonZeroUsize,
    max_rows: NonZeroUsize,
    max_response_size: NonZeroUsize,
}

/// PostgreSQL API exposed to Plaid modules.
pub struct Postgres {
    connections: HashMap<ConnectionName, PostgresConnection>,
}

#[derive(Debug, Error)]
pub enum PostgresError {
    #[error("invalid PostgreSQL configuration for connection [{0}]: {1}")]
    Configuration(String, String),
    #[error("PostgreSQL connection [{0}] is not configured")]
    UnknownConnection(String),
    #[error("module is not allowed to use PostgreSQL connection [{0}]")]
    ModuleNotPermitted(String),
    #[error("PostgreSQL pool error for connection [{0}]: {1}")]
    Pool(String, String),
    #[error("timed out waiting for PostgreSQL connection [{0}]")]
    PoolTimeout(String),
    #[error("PostgreSQL query exceeded its time limit")]
    QueryTimeout,
    #[error("PostgreSQL query is too large")]
    QueryTooLarge,
    #[error("PostgreSQL query has too many parameters")]
    TooManyParameters,
    #[error("PostgreSQL statement does not return rows")]
    NoResultColumns,
    #[error("PostgreSQL query returned more than the configured row limit")]
    TooManyRows,
    #[error("PostgreSQL response exceeded the configured size limit")]
    ResponseTooLarge,
    #[error("unsupported PostgreSQL type [{0}]; cast it to text or jsonb")]
    UnsupportedType(String),
    #[error("PostgreSQL returned a non-finite floating point value")]
    NonFiniteFloat,
    #[error("PostgreSQL error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("could not serialize PostgreSQL response: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl Postgres {
    pub async fn new(config: PostgresConfig) -> Result<Self, PostgresError> {
        let mut connections = HashMap::new();

        for (name, config) in config.connections {
            let manager = Manager::from_config(
                config.postgres_config,
                NoTls,
                ManagerConfig {
                    recycling_method: RecyclingMethod::Clean,
                },
            );
            let pool = Pool::builder(manager)
                .max_size(config.max_pool_size.get())
                .build()
                .map_err(|e| PostgresError::Configuration(name.to_string(), e.to_string()))?;

            let pool_timeout = Duration::from_millis(config.pool_timeout_ms.get());
            check_connectivity(&pool, pool_timeout, &name).await?;

            connections.insert(
                name,
                PostgresConnection {
                    pool,
                    allowed_rules: config.allowed_rules,
                    pool_timeout_ms: config.pool_timeout_ms,
                    statement_timeout_ms: config.statement_timeout_ms,
                    lock_timeout_ms: config.lock_timeout_ms,
                    max_query_size: config.max_query_size,
                    max_parameters: config.max_parameters,
                    max_rows: config.max_rows,
                    max_response_size: config.max_response_size,
                },
            );
        }

        Ok(Self { connections })
    }

    /// Execute one parameterized statement in a read-only transaction.
    pub async fn query(&self, params: &str, module: Arc<PlaidModule>) -> Result<String, ApiError> {
        let request =
            serde_json::from_str::<QueryRequest>(params).map_err(|_| ApiError::BadRequest)?;
        let connection = self
            .connections
            .get(request.connection.as_str())
            .ok_or_else(|| PostgresError::UnknownConnection(request.connection.clone()))?;

        if !module_is_allowed(&connection.allowed_rules, &module.name) {
            warn!(
                "[{module}] attempted to use PostgreSQL connection [{}] without permission",
                request.connection
            );
            return Err(PostgresError::ModuleNotPermitted(request.connection).into());
        }
        if request.sql.len() > connection.max_query_size.get() {
            return Err(PostgresError::QueryTooLarge.into());
        }
        if request.parameters.len() > connection.max_parameters.get() {
            return Err(PostgresError::TooManyParameters.into());
        }

        let pool_timeout = Duration::from_millis(connection.pool_timeout_ms.get());
        let mut client = tokio::time::timeout(pool_timeout, connection.pool.get())
            .await
            .map_err(|_| PostgresError::PoolTimeout(request.connection.clone()))?
            .map_err(|e| PostgresError::Pool(request.connection.clone(), e.to_string()))?;

        let transaction = client
            .build_transaction()
            .read_only(true)
            .start()
            .await
            .map_err(PostgresError::Database)?;

        let operation = async {
            transaction
                .batch_execute(&format!(
                    "SET LOCAL statement_timeout = '{}ms'; SET LOCAL lock_timeout = '{}ms';",
                    connection.statement_timeout_ms.get(),
                    connection.lock_timeout_ms.get()
                ))
                .await?;

            let statement = transaction.prepare(&request.sql).await?;
            if statement.columns().is_empty() {
                return Err(PostgresError::NoResultColumns);
            }

            let parameters: Vec<BoundParameter> =
                request.parameters.into_iter().map(BoundParameter).collect();
            let stream = transaction.query_raw(&statement, parameters.iter()).await?;
            pin_mut!(stream);

            let columns = statement
                .columns()
                .iter()
                .map(|column| QueryColumn {
                    name: column.name().to_string(),
                    postgres_type: column.type_().name().to_string(),
                })
                .collect::<Vec<_>>();
            let mut response_size = serde_json::to_vec(&columns)?.len();
            let mut rows = Vec::new();

            while let Some(row) = stream.try_next().await? {
                if rows.len() >= connection.max_rows.get() {
                    return Err(PostgresError::TooManyRows);
                }
                let decoded = decode_row(&row)?;
                response_size = response_size.saturating_add(serde_json::to_vec(&decoded)?.len());
                if response_size > connection.max_response_size.get() {
                    return Err(PostgresError::ResponseTooLarge);
                }
                rows.push(decoded);
            }

            let encoded = serde_json::to_string(&QueryResponse { columns, rows })?;
            if encoded.len() > connection.max_response_size.get() {
                return Err(PostgresError::ResponseTooLarge);
            }
            Ok(encoded)
        };

        // The server-side statement timeout is authoritative. The outer timeout
        // also bounds client-side waiting if the connection becomes unhealthy.
        let operation_result = tokio::time::timeout(
            Duration::from_millis(connection.statement_timeout_ms.get())
                .saturating_add(Duration::from_secs(1)),
            operation,
        )
        .await
        .map_err(|_| PostgresError::QueryTimeout);

        let rollback_result = transaction.rollback().await;
        let cleanup_result =
            tokio::time::timeout(pool_timeout, client.batch_execute(CLEAN_CONNECTION)).await;

        let result = operation_result?;
        if let Err(e) = rollback_result {
            return Err(PostgresError::Database(e).into());
        }
        match cleanup_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(PostgresError::Database(e).into()),
            Err(_) => return Err(PostgresError::QueryTimeout.into()),
        }

        result.map_err(ApiError::from)
    }
}

async fn check_connectivity(
    pool: &Pool,
    pool_timeout: Duration,
    connection_name: &ConnectionName,
) -> Result<(), PostgresError> {
    let client = tokio::time::timeout(pool_timeout, pool.get())
        .await
        .map_err(|_| PostgresError::PoolTimeout(connection_name.to_string()))?
        .map_err(|e| PostgresError::Pool(connection_name.to_string(), e.to_string()))?;
    client
        .simple_query("SELECT 1")
        .await
        .map_err(PostgresError::Database)?;

    Ok(())
}

fn module_is_allowed(allowed_rules: &HashSet<String>, module_name: &str) -> bool {
    allowed_rules.contains(module_name)
}

// Newtype wrapper to allow deriving ToSql on an STL type.
#[derive(Debug)]
struct BoundParameter(QueryParameter);

#[derive(Debug)]
struct ParameterTypeError {
    parameter: &'static str,
    postgres_type: String,
}

impl fmt::Display for ParameterTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} parameter cannot be bound as PostgreSQL type {}; add an explicit cast or use a matching parameter variant",
            self.parameter, self.postgres_type
        )
    }
}

impl Error for ParameterTypeError {}

impl BoundParameter {
    fn wrong_type(&self, postgres_type: &Type) -> ParameterTypeError {
        let parameter = match self.0 {
            QueryParameter::Null => "null",
            QueryParameter::Boolean(_) => "boolean",
            QueryParameter::Integer(_) => "integer",
            QueryParameter::Float(_) => "float",
            QueryParameter::String(_) => "string",
            QueryParameter::Bytes(_) => "bytes",
            QueryParameter::Json(_) => "JSON",
        };
        ParameterTypeError {
            parameter,
            postgres_type: postgres_type.name().to_string(),
        }
    }
}

impl ToSql for BoundParameter {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match &self.0 {
            QueryParameter::Null => Ok(IsNull::Yes),
            QueryParameter::Boolean(value) if *ty == Type::BOOL => value.to_sql(ty, out),
            QueryParameter::Integer(value) if *ty == Type::INT2 => i16::try_from(*value)
                .map_err(|_| self.wrong_type(ty))?
                .to_sql(ty, out),
            QueryParameter::Integer(value) if *ty == Type::INT4 => i32::try_from(*value)
                .map_err(|_| self.wrong_type(ty))?
                .to_sql(ty, out),
            QueryParameter::Integer(value) if *ty == Type::INT8 => value.to_sql(ty, out),
            QueryParameter::Float(value) if *ty == Type::FLOAT4 => (*value as f32).to_sql(ty, out),
            QueryParameter::Float(value) if *ty == Type::FLOAT8 => value.to_sql(ty, out),
            QueryParameter::String(value)
                if matches!(*ty, Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME) =>
            {
                value.to_sql(ty, out)
            }
            QueryParameter::Bytes(value) if *ty == Type::BYTEA => value.to_sql(ty, out),
            QueryParameter::Json(value) if matches!(*ty, Type::JSON | Type::JSONB) => {
                Json(value).to_sql(ty, out)
            }
            _ => Err(self.wrong_type(ty).into()),
        }
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

fn decode_row(row: &Row) -> Result<Vec<Value>, PostgresError> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(index, column)| decode_cell(row, index, column.type_()))
        .collect()
}

fn decode_cell(row: &Row, index: usize, ty: &Type) -> Result<Value, PostgresError> {
    macro_rules! decode_optional {
        ($rust_type:ty, $map:expr) => {{
            let value = row.try_get::<_, Option<$rust_type>>(index)?;
            Ok(value.map($map).unwrap_or(Value::Null))
        }};
    }

    match *ty {
        Type::BOOL => decode_optional!(bool, Value::Bool),
        Type::CHAR => decode_optional!(i8, |value| Value::from(value as i64)),
        Type::INT2 => decode_optional!(i16, |value| Value::from(value as i64)),
        Type::INT4 => decode_optional!(i32, |value| Value::from(value as i64)),
        Type::INT8 => decode_optional!(i64, Value::from),
        Type::FLOAT4 => decode_float(row.try_get::<_, Option<f32>>(index)?.map(f64::from)),
        Type::FLOAT8 => decode_float(row.try_get::<_, Option<f64>>(index)?),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => {
            decode_optional!(String, Value::String)
        }
        Type::BYTEA => decode_optional!(Vec<u8>, |value| serde_json::to_value(value)
            .expect("Vec<u8> serialization cannot fail")),
        Type::JSON | Type::JSONB => {
            let value = row.try_get::<_, Option<Json<Value>>>(index)?;
            Ok(value.map(|value| value.0).unwrap_or(Value::Null))
        }
        _ => Err(PostgresError::UnsupportedType(ty.name().to_string())),
    }
}

fn decode_float(value: Option<f64>) -> Result<Value, PostgresError> {
    match value {
        None => Ok(Value::Null),
        Some(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or(PostgresError::NonFiniteFloat),
    }
}

fn default_max_pool_size() -> NonZeroUsize {
    NonZeroUsize::new(4).expect("default PostgreSQL pool size must be non-zero")
}

fn default_pool_timeout_ms() -> NonZeroU64 {
    NonZeroU64::new(1_000).expect("default PostgreSQL pool timeout must be non-zero")
}

fn default_statement_timeout_ms() -> NonZeroU64 {
    NonZeroU64::new(5_000).expect("default PostgreSQL statement timeout must be non-zero")
}

fn default_lock_timeout_ms() -> NonZeroU64 {
    NonZeroU64::new(1_000).expect("default PostgreSQL lock timeout must be non-zero")
}

fn default_max_query_size() -> NonZeroUsize {
    NonZeroUsize::new(64 * 1024).expect("default PostgreSQL query size must be non-zero")
}

fn default_max_parameters() -> NonZeroUsize {
    NonZeroUsize::new(100).expect("default PostgreSQL parameter limit must be non-zero")
}

fn default_max_rows() -> NonZeroUsize {
    NonZeroUsize::new(1_000).expect("default PostgreSQL row limit must be non-zero")
}

fn default_max_response_size() -> NonZeroUsize {
    NonZeroUsize::new(1024 * 1024).expect("default PostgreSQL response size must be non-zero")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "cranelift")]
    use crate::loader::LimitValue;
    #[cfg(feature = "cranelift")]
    use wasmer::{
        sys::{Cranelift, EngineBuilder},
        Module, Store,
    };

    #[test]
    fn configuration_defaults_and_permissions() {
        let config: PostgresConfig = toml::from_str(
            r#"
            [connections.local]
            connection_string = "host=localhost user=reader"
            allowed_rules = ["reader.wasm"]
            "#,
        )
        .unwrap();
        let local = config.connections.get("local").unwrap();

        assert_eq!(local.max_pool_size, default_max_pool_size());
        assert_eq!(local.max_rows, default_max_rows());
        assert!(module_is_allowed(&local.allowed_rules, "reader.wasm"));
        assert!(!module_is_allowed(&local.allowed_rules, "other.wasm"));
    }

    #[test]
    fn zero_limits_are_rejected() {
        let error = toml::from_str::<PostgresConfig>(
            r#"
            [connections.local]
            connection_string = "host=localhost user=reader"
            allowed_rules = ["reader.wasm"]
            max_rows = 0
            "#,
        )
        .err()
        .expect("zero-valued limits must fail during deserialization");

        assert!(error.to_string().contains("nonzero usize"));
    }

    #[test]
    fn invalid_connection_names_and_strings_are_rejected() {
        let empty_name = toml::from_str::<PostgresConfig>(
            r#"
            [connections.""]
            connection_string = "host=localhost user=reader"
            allowed_rules = []
            "#,
        )
        .err()
        .expect("empty connection names must fail during deserialization");
        assert!(empty_name
            .to_string()
            .contains("PostgreSQL connection name cannot be empty"));

        toml::from_str::<PostgresConfig>(
            r#"
            [connections.local]
            connection_string = "not a valid connection option"
            allowed_rules = []
            "#,
        )
        .err()
        .expect("invalid connection strings must fail during deserialization");
    }

    #[test]
    fn common_parameters_encode_for_expected_types() {
        let cases = [
            (BoundParameter(QueryParameter::Boolean(true)), Type::BOOL),
            (BoundParameter(QueryParameter::Integer(42)), Type::INT4),
            (BoundParameter(QueryParameter::Float(1.5)), Type::FLOAT8),
            (
                BoundParameter(QueryParameter::String("hello".to_string())),
                Type::TEXT,
            ),
            (
                BoundParameter(QueryParameter::Json(serde_json::json!({"ok": true}))),
                Type::JSONB,
            ),
        ];

        for (parameter, ty) in cases {
            let mut output = BytesMut::new();
            assert!(parameter.to_sql_checked(&ty, &mut output).is_ok());
        }

        let mut output = BytesMut::new();
        assert!(BoundParameter(QueryParameter::Integer(42))
            .to_sql_checked(&Type::TEXT, &mut output)
            .is_err());
    }

    #[cfg(feature = "cranelift")]
    fn test_module(name: &str) -> Arc<PlaidModule> {
        let store = Store::default();
        let wasm = &[0, 97, 115, 109, 1, 0, 0, 0];
        let engine = EngineBuilder::new(Cranelift::default());

        Arc::new(PlaidModule {
            name: name.to_string(),
            logtype: "test".to_string(),
            module: Module::new(&store, wasm).unwrap(),
            engine: engine.into(),
            computation_limit: 0,
            page_limit: 0,
            storage_current: Default::default(),
            storage_limit: LimitValue::Unlimited,
            accessory_data: None,
            secrets: None,
            persistent_response: None,
            test_mode: true,
        })
    }

    /// Optional live smoke test. The supplied role must be able to create the
    /// fixture table outside the API; the API itself still executes in a
    /// read-only transaction.
    #[cfg(feature = "cranelift")]
    #[tokio::test]
    #[ignore = "requires PLAID_TEST_POSTGRES_URL"]
    async fn live_query_and_read_only_enforcement() {
        let connection_string = std::env::var("PLAID_TEST_POSTGRES_URL")
            .expect("PLAID_TEST_POSTGRES_URL must be set for this ignored test");
        let (admin, connection) = tokio_postgres::connect(&connection_string, NoTls)
            .await
            .unwrap();
        let connection_task = tokio::spawn(async move { connection.await.unwrap() });
        admin
            .batch_execute(
                "DROP TABLE IF EXISTS plaid_postgres_api_test;\
                 CREATE TABLE plaid_postgres_api_test (id bigint PRIMARY KEY, payload jsonb);\
                 INSERT INTO plaid_postgres_api_test VALUES (1, '{\"ok\": true}');",
            )
            .await
            .unwrap();

        let config = PostgresConfig {
            connections: HashMap::from([(
                ConnectionName::new("test".to_string()).unwrap(),
                PostgresConnectionConfig {
                    postgres_config: tokio_postgres::Config::from_str(&connection_string).unwrap(),
                    allowed_rules: HashSet::from(["reader.wasm".to_string()]),
                    max_pool_size: NonZeroUsize::new(2).unwrap(),
                    pool_timeout_ms: NonZeroU64::new(2_000).unwrap(),
                    statement_timeout_ms: NonZeroU64::new(2_000).unwrap(),
                    lock_timeout_ms: NonZeroU64::new(1_000).unwrap(),
                    max_query_size: default_max_query_size(),
                    max_parameters: default_max_parameters(),
                    max_rows: default_max_rows(),
                    max_response_size: default_max_response_size(),
                },
            )]),
        };
        let postgres = Postgres::new(config).await.unwrap();
        let module = test_module("reader.wasm");

        let request = QueryRequest {
            connection: "test".to_string(),
            sql: "SELECT id, payload FROM plaid_postgres_api_test WHERE id = $1::bigint"
                .to_string(),
            parameters: vec![QueryParameter::Integer(1)],
        };
        let response = postgres
            .query(&serde_json::to_string(&request).unwrap(), module.clone())
            .await
            .unwrap();
        let response: QueryResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(response.rows[0][0], Value::from(1));
        assert_eq!(response.rows[0][1], serde_json::json!({"ok": true}));

        let write = QueryRequest {
            connection: "test".to_string(),
            sql: "INSERT INTO plaid_postgres_api_test VALUES (2, '{}') RETURNING id".to_string(),
            parameters: vec![],
        };
        assert!(postgres
            .query(&serde_json::to_string(&write).unwrap(), module)
            .await
            .is_err());

        drop(postgres);
        admin
            .batch_execute("DROP TABLE plaid_postgres_api_test")
            .await
            .unwrap();
        drop(admin);
        connection_task.await.unwrap();
    }
}
