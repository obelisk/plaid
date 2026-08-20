use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    io::BufReader,
    num::{NonZeroU64, NonZeroUsize},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use deadpool_postgres::{
    Client, Manager, ManagerConfig, Pool, PoolError, RecyclingMethod, Runtime, TimeoutType,
    Timeouts,
};
use futures_util::{pin_mut, TryStreamExt};
use plaid_stl::postgres::{QueryColumn, QueryParameter, QueryRequest, QueryResponse};
use rustls::{pki_types::PrivateKeyDer, ClientConfig, RootCertStore};
use rustls_pemfile::Item;
use serde::{de::Error as _, Deserialize, Deserializer};
use serde_json::Value;
use thiserror::Error;
use tokio::time::Instant;
use tokio_postgres::{
    config::SslMode,
    types::{to_sql_checked, IsNull, Json, ToSql, Type},
    NoTls, Row,
};
use tokio_postgres_rustls::MakeRustlsConnect;
use tokio_util::bytes::BytesMut;

use crate::{apis::ApiError, cryptography::hash::sha256_hex, loader::PlaidModule};

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
    tokio_postgres::Config::from_str(&connection_string)
        .map_err(|_| D::Error::custom("invalid PostgreSQL connection string"))
}

/// Configuration for all named PostgreSQL connections.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresConfig {
    connections: HashMap<ConnectionName, PostgresConnectionConfig>,
}

/// Configuration and safety limits for one PostgreSQL connection.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PostgresConnectionConfig {
    #[serde(
        rename = "connection_string",
        deserialize_with = "deserialize_connection_string"
    )]
    postgres_config: tokio_postgres::Config,
    allowed_rules: HashSet<String>,
    tls: PostgresTlsConfig,
    #[serde(default = "default_max_pool_size")]
    max_pool_size: NonZeroUsize,
    #[serde(default = "default_pool_timeout_ms")]
    pool_timeout_ms: NonZeroU64,
    #[serde(default = "default_connection_timeout_ms")]
    connection_timeout_ms: NonZeroU64,
    #[serde(default = "default_recycle_timeout_ms")]
    recycle_timeout_ms: NonZeroU64,
    #[serde(default = "default_total_timeout_ms")]
    total_timeout_ms: NonZeroU64,
    #[serde(default = "default_statement_timeout_ms")]
    statement_timeout_ms: NonZeroU64,
    #[serde(default = "default_lock_timeout_ms")]
    lock_timeout_ms: NonZeroU64,
    #[serde(default = "default_idle_transaction_timeout_ms")]
    idle_transaction_timeout_ms: NonZeroU64,
    #[serde(default = "default_max_query_size")]
    max_query_size: NonZeroUsize,
    #[serde(default = "default_max_parameters")]
    max_parameters: NonZeroUsize,
    #[serde(default = "default_max_rows")]
    max_rows: NonZeroUsize,
    #[serde(default = "default_max_response_size")]
    max_response_size: NonZeroUsize,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PostgresTlsConfig {
    mode: PostgresTlsMode,
    #[serde(default = "default_use_system_roots")]
    use_system_roots: bool,
    ca_certificates_pem: Option<String>,
    client_certificate_pem: Option<String>,
    client_private_key_pem: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PostgresTlsMode {
    VerifyFull,
    Disable,
}

struct PostgresConnection {
    pool: Pool,
    allowed_rules: HashSet<String>,
    total_timeout_ms: NonZeroU64,
    statement_timeout_ms: NonZeroU64,
    lock_timeout_ms: NonZeroU64,
    idle_transaction_timeout_ms: NonZeroU64,
    max_query_size: NonZeroUsize,
    max_parameters: NonZeroUsize,
    max_rows: NonZeroUsize,
    max_response_size: NonZeroUsize,
}

/// PostgreSQL API exposed to Plaid modules.
pub struct Postgres {
    connections: HashMap<ConnectionName, PostgresConnection>,
}

#[derive(Error)]
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
    #[error("timed out creating PostgreSQL connection [{0}]")]
    ConnectionTimeout(String),
    #[error("timed out recycling PostgreSQL connection [{0}]")]
    RecycleTimeout(String),
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

impl fmt::Debug for PostgresError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // PostgreSQL diagnostics can contain parameter or row values (for
            // example, a failed cast). The generic API boundary logs errors
            // with `Debug`, so do not expose the driver's diagnostic fields.
            Self::Database(_) => formatter.write_str("PostgresError::Database(<redacted>)"),
            _ => fmt::Display::fmt(self, formatter),
        }
    }
}

impl PostgresError {
    fn outcome(&self) -> &'static str {
        match self {
            Self::Configuration(_, _) => "configuration_error",
            Self::UnknownConnection(_) => "unknown_connection",
            Self::ModuleNotPermitted(_) => "unauthorized",
            Self::Pool(_, _) => "pool_error",
            Self::PoolTimeout(_) => "pool_timeout",
            Self::ConnectionTimeout(_) => "connection_timeout",
            Self::RecycleTimeout(_) => "recycle_timeout",
            Self::QueryTimeout => "operation_timeout",
            Self::QueryTooLarge => "query_limit",
            Self::TooManyParameters => "parameter_limit",
            Self::NoResultColumns => "no_result_columns",
            Self::TooManyRows => "row_limit",
            Self::ResponseTooLarge => "response_limit",
            Self::UnsupportedType(_) => "unsupported_type",
            Self::NonFiniteFloat => "non_finite_float",
            Self::Database(_) => "database_error",
            Self::Serialization(_) => "serialization_error",
        }
    }
}

impl Postgres {
    /// Build pools for all configured PostgreSQL connections without opening
    /// them. Connections are established lazily by [`Self::query`], so an
    /// unavailable database cannot prevent Plaid from starting and can recover
    /// without requiring a Plaid restart.
    pub fn new(config: PostgresConfig) -> Result<Self, PostgresError> {
        let mut connections = HashMap::new();

        for (name, mut config) in config.connections {
            configure_postgres_connection(&name, &mut config.postgres_config, config.tls.mode);
            let timeouts = pool_timeouts(&config);

            let manager_config = ManagerConfig {
                recycling_method: RecyclingMethod::Clean,
            };
            let manager = match config.tls.mode {
                PostgresTlsMode::VerifyFull => Manager::from_config(
                    config.postgres_config,
                    build_tls_connector(&name, &config.tls)?,
                    manager_config,
                ),
                PostgresTlsMode::Disable => {
                    validate_disabled_tls(&name, &config.tls)?;
                    warn!(
                        "PostgreSQL connection [{name}] has TLS disabled; plaintext is intended for local development only"
                    );
                    Manager::from_config(config.postgres_config, NoTls, manager_config)
                }
            };
            let pool = Pool::builder(manager)
                .max_size(config.max_pool_size.get())
                .timeouts(timeouts)
                .runtime(Runtime::Tokio1)
                .build()
                .map_err(|e| PostgresError::Configuration(name.to_string(), e.to_string()))?;

            connections.insert(
                name,
                PostgresConnection {
                    pool,
                    allowed_rules: config.allowed_rules,
                    total_timeout_ms: config.total_timeout_ms,
                    statement_timeout_ms: config.statement_timeout_ms,
                    lock_timeout_ms: config.lock_timeout_ms,
                    idle_transaction_timeout_ms: config.idle_transaction_timeout_ms,
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
        let started = Instant::now();
        let connection_name = request.connection.clone();
        let fingerprint = sha256_hex(request.sql.as_bytes());
        let result = self.query_inner(request, &module.name).await;
        let (outcome, row_count) = match &result {
            Ok(success) => ("success", success.row_count),
            Err(error) => (error.outcome(), 0),
        };
        info!(
            "PostgreSQL query connection=[{}] module=[{}] elapsed_ms={} rows={} fingerprint={} outcome={}",
            connection_name,
            module.name,
            started.elapsed().as_millis(),
            row_count,
            fingerprint,
            outcome,
        );

        result
            .map(|success| success.response)
            .map_err(ApiError::from)
    }

    async fn query_inner(
        &self,
        request: QueryRequest,
        module_name: &str,
    ) -> Result<QuerySuccess, PostgresError> {
        let connection_name = request.connection.clone();
        let connection = self
            .connections
            .get(connection_name.as_str())
            .ok_or_else(|| PostgresError::UnknownConnection(connection_name.clone()))?;

        if !module_is_allowed(&connection.allowed_rules, module_name) {
            warn!(
                "module [{module_name}] attempted to use PostgreSQL connection [{connection_name}] without permission"
            );
            return Err(PostgresError::ModuleNotPermitted(connection_name));
        }
        if request.sql.len() > connection.max_query_size.get() {
            return Err(PostgresError::QueryTooLarge);
        }
        if request.parameters.len() > connection.max_parameters.get() {
            return Err(PostgresError::TooManyParameters);
        }

        let deadline = Instant::now() + Duration::from_millis(connection.total_timeout_ms.get());
        let mut client = tokio::time::timeout_at(deadline, connection.pool.get())
            .await
            .map_err(|_| PostgresError::QueryTimeout)?
            .map_err(|error| map_pool_error(&connection_name, error))?;

        match tokio::time::timeout_at(
            deadline,
            execute_read_only(&mut client, connection, request),
        )
        .await
        {
            Ok(Ok(success)) => Ok(QuerySuccess {
                response: success.response,
                row_count: success.row_count,
            }),
            Ok(Err(failure)) => {
                if failure.discard_connection {
                    discard_connection(client);
                }
                Err(failure.error)
            }
            Err(_) => {
                discard_connection(client);
                Err(PostgresError::QueryTimeout)
            }
        }
    }
}

fn pool_timeouts(config: &PostgresConnectionConfig) -> Timeouts {
    Timeouts {
        wait: Some(Duration::from_millis(config.pool_timeout_ms.get())),
        create: Some(Duration::from_millis(config.connection_timeout_ms.get())),
        recycle: Some(Duration::from_millis(config.recycle_timeout_ms.get())),
    }
}

fn configure_postgres_connection(
    name: &ConnectionName,
    postgres_config: &mut tokio_postgres::Config,
    tls_mode: PostgresTlsMode,
) {
    postgres_config.application_name(format!("plaid:postgres:{name}"));
    postgres_config.ssl_mode(match tls_mode {
        PostgresTlsMode::VerifyFull => SslMode::Require,
        PostgresTlsMode::Disable => SslMode::Disable,
    });
}

struct QuerySuccess {
    response: String,
    row_count: usize,
}

struct OperationFailure {
    error: PostgresError,
    discard_connection: bool,
}

async fn execute_read_only(
    client: &mut Client,
    connection: &PostgresConnection,
    request: QueryRequest,
) -> Result<QuerySuccess, OperationFailure> {
    let transaction = client
        .build_transaction()
        .read_only(true)
        .start()
        .await
        .map_err(|error| OperationFailure {
            error: PostgresError::Database(error),
            discard_connection: false,
        })?;

    let operation = async {
        transaction
            .batch_execute(&format!(
                "SET LOCAL statement_timeout = '{}ms';\
                 SET LOCAL lock_timeout = '{}ms';\
                 SET LOCAL idle_in_transaction_session_timeout = '{}ms';",
                connection.statement_timeout_ms.get(),
                connection.lock_timeout_ms.get(),
                connection.idle_transaction_timeout_ms.get(),
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

        let row_count = rows.len();
        let response = serde_json::to_string(&QueryResponse { columns, rows })?;
        if response.len() > connection.max_response_size.get() {
            return Err(PostgresError::ResponseTooLarge);
        }
        Ok((response, row_count))
    }
    .await;

    if let Err(error) = transaction.rollback().await {
        return Err(OperationFailure {
            error: PostgresError::Database(error),
            discard_connection: true,
        });
    }

    operation
        .map(|(response, row_count)| QuerySuccess {
            response,
            row_count,
        })
        .map_err(|error| OperationFailure {
            error,
            discard_connection: false,
        })
}

fn discard_connection(client: Client) {
    drop(Client::take(client));
}

fn map_pool_error(connection: &str, error: PoolError) -> PostgresError {
    match error {
        PoolError::Timeout(TimeoutType::Wait) => PostgresError::PoolTimeout(connection.to_string()),
        PoolError::Timeout(TimeoutType::Create) => {
            PostgresError::ConnectionTimeout(connection.to_string())
        }
        PoolError::Timeout(TimeoutType::Recycle) => {
            PostgresError::RecycleTimeout(connection.to_string())
        }
        error => PostgresError::Pool(connection.to_string(), error.to_string()),
    }
}

fn validate_disabled_tls(
    name: &ConnectionName,
    tls: &PostgresTlsConfig,
) -> Result<(), PostgresError> {
    if tls.ca_certificates_pem.is_some()
        || tls.client_certificate_pem.is_some()
        || tls.client_private_key_pem.is_some()
    {
        return Err(configuration_error(
            name,
            "certificate fields are not allowed when TLS mode is disable",
        ));
    }
    Ok(())
}

fn build_tls_connector(
    name: &ConnectionName,
    tls: &PostgresTlsConfig,
) -> Result<MakeRustlsConnect, PostgresError> {
    let mut roots = RootCertStore::empty();
    if tls.use_system_roots {
        let native = rustls_native_certs::load_native_certs();
        if !native.errors.is_empty() {
            warn!(
                "encountered {} non-fatal error(s) while loading system roots for PostgreSQL connection [{name}]",
                native.errors.len()
            );
        }
        for certificate in native.certs {
            roots.add(certificate).map_err(|_| {
                configuration_error(
                    name,
                    "the system trust store contains an invalid certificate",
                )
            })?;
        }
    }

    if let Some(pem) = &tls.ca_certificates_pem {
        for certificate in parse_certificate_bundle(name, pem, "CA certificate bundle")? {
            roots.add(certificate).map_err(|_| {
                configuration_error(
                    name,
                    "the CA certificate bundle contains an invalid certificate",
                )
            })?;
        }
    }
    if roots.is_empty() {
        return Err(configuration_error(
            name,
            "verify_full requires at least one system or custom CA certificate",
        ));
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| configuration_error(name, "could not initialize the TLS protocol versions"))?
        .with_root_certificates(roots);

    let client_config = match (&tls.client_certificate_pem, &tls.client_private_key_pem) {
        (None, None) => builder.with_no_client_auth(),
        (Some(certificate_pem), Some(private_key_pem)) => builder
            .with_client_auth_cert(
                parse_certificate_bundle(name, certificate_pem, "client certificate bundle")?,
                parse_private_key(name, private_key_pem)?,
            )
            .map_err(|_| {
                configuration_error(
                    name,
                    "the client certificate and private key are invalid or do not match",
                )
            })?,
        _ => {
            return Err(configuration_error(
                name,
                "client_certificate_pem and client_private_key_pem must be configured together",
            ))
        }
    };

    Ok(MakeRustlsConnect::new(client_config))
}

fn parse_certificate_bundle(
    name: &ConnectionName,
    pem: &str,
    description: &str,
) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, PostgresError> {
    let mut reader = BufReader::new(pem.as_bytes());
    let items = rustls_pemfile::read_all(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| configuration_error(name, &format!("malformed {description}")))?;
    let mut certificates = Vec::new();
    for item in items {
        match item {
            Item::X509Certificate(certificate) => certificates.push(certificate),
            _ => {
                return Err(configuration_error(
                    name,
                    &format!("{description} contains non-certificate PEM material"),
                ))
            }
        }
    }
    if certificates.is_empty() {
        return Err(configuration_error(name, &format!("empty {description}")));
    }
    Ok(certificates)
}

fn parse_private_key(
    name: &ConnectionName,
    pem: &str,
) -> Result<PrivateKeyDer<'static>, PostgresError> {
    let mut reader = BufReader::new(pem.as_bytes());
    let items = rustls_pemfile::read_all(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| configuration_error(name, "malformed client private key"))?;
    let mut keys = items.into_iter().map(|item| match item {
        Item::Pkcs1Key(key) => Ok(PrivateKeyDer::Pkcs1(key)),
        Item::Pkcs8Key(key) => Ok(PrivateKeyDer::Pkcs8(key)),
        Item::Sec1Key(key) => Ok(PrivateKeyDer::Sec1(key)),
        _ => Err(configuration_error(
            name,
            "client private key contains unsupported PEM material",
        )),
    });
    let key = keys
        .next()
        .transpose()?
        .ok_or_else(|| configuration_error(name, "empty or unsupported client private key"))?;
    if keys.next().transpose()?.is_some() {
        return Err(configuration_error(
            name,
            "client_private_key_pem must contain exactly one private key",
        ));
    }
    Ok(key)
}

fn configuration_error(name: &ConnectionName, message: &str) -> PostgresError {
    PostgresError::Configuration(name.to_string(), message.to_string())
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

fn default_connection_timeout_ms() -> NonZeroU64 {
    NonZeroU64::new(3_000).expect("default PostgreSQL connection timeout must be non-zero")
}

fn default_recycle_timeout_ms() -> NonZeroU64 {
    NonZeroU64::new(1_000).expect("default PostgreSQL recycle timeout must be non-zero")
}

fn default_total_timeout_ms() -> NonZeroU64 {
    NonZeroU64::new(7_000).expect("default PostgreSQL total timeout must be non-zero")
}

fn default_statement_timeout_ms() -> NonZeroU64 {
    NonZeroU64::new(5_000).expect("default PostgreSQL statement timeout must be non-zero")
}

fn default_lock_timeout_ms() -> NonZeroU64 {
    NonZeroU64::new(1_000).expect("default PostgreSQL lock timeout must be non-zero")
}

fn default_idle_transaction_timeout_ms() -> NonZeroU64 {
    NonZeroU64::new(5_000).expect("default PostgreSQL idle transaction timeout must be non-zero")
}

fn default_use_system_roots() -> bool {
    true
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

    #[test]
    fn configuration_defaults_and_permissions() {
        let config: PostgresConfig = toml::from_str(
            r#"
            [connections.local]
            connection_string = "host=localhost user=reader"
            allowed_rules = ["reader.wasm"]

            [connections.local.tls]
            mode = "disable"
            "#,
        )
        .unwrap();
        let local = config.connections.get("local").unwrap();

        assert_eq!(local.max_pool_size, default_max_pool_size());
        assert_eq!(local.max_rows, default_max_rows());
        assert!(local.tls.use_system_roots);
        assert!(module_is_allowed(&local.allowed_rules, "reader.wasm"));
        assert!(!module_is_allowed(&local.allowed_rules, "other.wasm"));

        let timeouts = pool_timeouts(local);
        assert_eq!(
            timeouts.wait,
            Some(Duration::from_millis(default_pool_timeout_ms().get()))
        );
        assert_eq!(
            timeouts.create,
            Some(Duration::from_millis(default_connection_timeout_ms().get()))
        );
        assert_eq!(
            timeouts.recycle,
            Some(Duration::from_millis(default_recycle_timeout_ms().get()))
        );
    }

    #[test]
    fn tls_stanza_and_mode_are_required() {
        let missing_tls = toml::from_str::<PostgresConfig>(
            r#"
            [connections.local]
            connection_string = "host=localhost user=reader"
            allowed_rules = []
            "#,
        )
        .err()
        .expect("the TLS stanza must be explicit");
        assert!(missing_tls.to_string().contains("missing field `tls`"));

        let missing_mode = toml::from_str::<PostgresConfig>(
            r#"
            [connections.local]
            connection_string = "host=localhost user=reader"
            allowed_rules = []

            [connections.local.tls]
            use_system_roots = true
            "#,
        )
        .err()
        .expect("the TLS mode must be explicit");
        assert!(missing_mode.to_string().contains("missing field `mode`"));
    }

    #[test]
    fn unknown_configuration_fields_are_rejected() {
        let unknown_top_level = toml::from_str::<PostgresConfig>(
            r#"
            unexpected = true

            [connections.local]
            connection_string = "host=localhost user=reader"
            allowed_rules = []

            [connections.local.tls]
            mode = "disable"
            "#,
        )
        .err()
        .expect("unknown top-level fields must fail during deserialization");
        assert!(unknown_top_level
            .to_string()
            .contains("unknown field `unexpected`"));

        let unknown_connection_field = toml::from_str::<PostgresConfig>(
            r#"
            [connections.local]
            connection_string = "host=localhost user=reader"
            allowed_rules = []
            max_respose_size = 1024

            [connections.local.tls]
            mode = "disable"
            "#,
        )
        .err()
        .expect("unknown connection fields must fail during deserialization");
        assert!(unknown_connection_field
            .to_string()
            .contains("unknown field `max_respose_size`"));
    }

    #[test]
    fn tls_stanza_overrides_dsn_sslmode_and_sets_application_name() {
        let name = ConnectionName::new("corporate".to_string()).unwrap();
        let mut plaintext_dsn =
            tokio_postgres::Config::from_str("host=localhost sslmode=require").unwrap();
        configure_postgres_connection(&name, &mut plaintext_dsn, PostgresTlsMode::Disable);
        assert_eq!(plaintext_dsn.get_ssl_mode(), SslMode::Disable);
        assert_eq!(
            plaintext_dsn.get_application_name(),
            Some("plaid:postgres:corporate")
        );

        let mut verified_dsn =
            tokio_postgres::Config::from_str("host=localhost sslmode=disable").unwrap();
        configure_postgres_connection(&name, &mut verified_dsn, PostgresTlsMode::VerifyFull);
        assert_eq!(verified_dsn.get_ssl_mode(), SslMode::Require);
    }

    #[test]
    fn plaintext_rejects_certificate_fields() {
        let name = ConnectionName::new("local".to_string()).unwrap();
        let tls = PostgresTlsConfig {
            mode: PostgresTlsMode::Disable,
            use_system_roots: true,
            ca_certificates_pem: Some(String::new()),
            client_certificate_pem: None,
            client_private_key_pem: None,
        };
        let error = validate_disabled_tls(&name, &tls).unwrap_err();
        assert!(error
            .to_string()
            .contains("certificate fields are not allowed"));
    }

    #[test]
    fn verify_full_requires_roots_and_validates_client_pair() {
        let name = ConnectionName::new("production".to_string()).unwrap();
        let no_roots = PostgresTlsConfig {
            mode: PostgresTlsMode::VerifyFull,
            use_system_roots: false,
            ca_certificates_pem: None,
            client_certificate_pem: None,
            client_private_key_pem: None,
        };
        assert!(build_tls_connector(&name, &no_roots)
            .err()
            .unwrap()
            .to_string()
            .contains("requires at least one"));

        let rcgen::CertifiedKey { cert, key_pair } =
            rcgen::generate_simple_self_signed(["postgres.example.com".to_string()]).unwrap();
        let cert_pem = cert.pem();
        let key_pem = key_pair.serialize_pem();
        let missing_key = PostgresTlsConfig {
            mode: PostgresTlsMode::VerifyFull,
            use_system_roots: false,
            ca_certificates_pem: Some(cert_pem.clone()),
            client_certificate_pem: Some(cert_pem.clone()),
            client_private_key_pem: None,
        };
        assert!(build_tls_connector(&name, &missing_key)
            .err()
            .unwrap()
            .to_string()
            .contains("must be configured together"));

        let valid_mtls = PostgresTlsConfig {
            mode: PostgresTlsMode::VerifyFull,
            use_system_roots: false,
            ca_certificates_pem: Some(cert_pem.clone()),
            client_certificate_pem: Some(cert_pem),
            client_private_key_pem: Some(key_pem),
        };
        build_tls_connector(&name, &valid_mtls).unwrap();
    }

    #[test]
    fn malformed_or_ambiguous_pem_is_rejected_without_echoing_it() {
        let name = ConnectionName::new("production".to_string()).unwrap();
        let secret_marker = "SHOULD-NOT-APPEAR";
        let error = parse_certificate_bundle(
            &name,
            &format!("-----BEGIN CERTIFICATE-----\n{secret_marker}\n"),
            "CA certificate bundle",
        )
        .unwrap_err();
        assert!(!error.to_string().contains(secret_marker));

        let rcgen::CertifiedKey { key_pair, .. } =
            rcgen::generate_simple_self_signed(["client".to_string()]).unwrap();
        let key = key_pair.serialize_pem();
        let error = parse_private_key(&name, &format!("{key}\n{key}"))
            .expect_err("multiple keys must be rejected");
        assert!(error.to_string().contains("exactly one"));
    }

    #[test]
    fn zero_limits_are_rejected() {
        let error = toml::from_str::<PostgresConfig>(
            r#"
            [connections.local]
            connection_string = "host=localhost user=reader"
            allowed_rules = ["reader.wasm"]
            max_rows = 0

            [connections.local.tls]
            mode = "disable"
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

            [connections."".tls]
            mode = "disable"
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

            [connections.local.tls]
            mode = "disable"
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

    #[test]
    fn database_error_debug_output_is_redacted() {
        let secret_marker = "SHOULD-NOT-APPEAR";
        let driver_error = tokio_postgres::Config::from_str(&format!(
            "host=localhost password={secret_marker} invalid-option"
        ))
        .expect_err("the connection string must be invalid");
        let error = ApiError::from(PostgresError::Database(driver_error));
        let debug_output = format!("{error:?}");

        assert_eq!(
            debug_output,
            "PostgresError(PostgresError::Database(<redacted>))"
        );
        assert!(!debug_output.contains(secret_marker));
    }
}
