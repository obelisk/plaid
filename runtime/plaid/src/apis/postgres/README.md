# Production PostgreSQL API

The PostgreSQL API is a feature-gated, read-only interface for curated SQL in
Plaid rules. Treat rule-supplied SQL as trusted application code: review it,
use deterministic ordering, bound the number of rows, and bound large text,
byte, and JSON values in SQL.

## Plaid configuration

Every connection requires an explicit TLS stanza. `verify_full` requires TLS,
validates the server chain, and verifies the connection hostname against the
certificate. The TLS stanza overrides `sslmode` in the connection string.
`disable` is retained only for explicit local development and emits a startup
warning.

```toml
[apis.postgres.connections.corporate]
connection_string = "{plaid-secret{postgres-corporate-url}}"
allowed_rules = ["reader.wasm"]
max_pool_size = 4
pool_timeout_ms = 1000
connection_timeout_ms = 3000
recycle_timeout_ms = 1000
total_timeout_ms = 7000
statement_timeout_ms = 5000
lock_timeout_ms = 1000
idle_transaction_timeout_ms = 5000
max_query_size = 65536
max_parameters = 100
max_rows = 1000
max_response_size = 1048576

[apis.postgres.connections.corporate.tls]
mode = "verify_full"
use_system_roots = true
ca_certificates_pem = """{plaid-secret{postgres-corporate-ca}}"""
client_certificate_pem = """{plaid-secret{postgres-client-certificate}}"""
client_private_key_pem = """{plaid-secret{postgres-client-private-key}}"""
```

`ca_certificates_pem` appends one or more private CA certificates to the trust
store. Set `use_system_roots = false` to trust only that bundle. The client
certificate and unencrypted PKCS#1, PKCS#8, or SEC1 private key are optional,
but must be provided together. mTLS can be used with a password/SCRAM DSN or as
the server's authentication method. Changing a DSN, password, CA, certificate,
or key requires a controlled Plaid restart.

Use a DNS hostname present in the server certificate SAN; hostname overrides,
opportunistic TLS, verification bypass, and TLS-without-verification are not
supported. Prefer a read replica or other server-enforced read-only endpoint.

Each request runs in an explicit read-only transaction with local statement,
lock, and idle-in-transaction timeouts. Pool waiting, connection creation,
recycling, and the total operation are independently bounded. A timed-out or
uncertain connection is discarded. Deadpool's clean recycler resets sessions
before reuse.

The response limit bounds serialized JSON. It cannot stop PostgreSQL from
transmitting one oversized row, so production queries must truncate, reject,
or omit potentially large text, bytea, JSON, and aggregate columns.

The API decodes null, boolean, integer, floating-point, text, bytea, JSON, and
JSONB scalars. Curated SQL must explicitly cast UUID, numeric, temporal, array,
enum, domain, and extension types to `text` or `jsonb`; Plaid intentionally does
not make implicit precision or representation choices.

## Database role

Create a dedicated login that cannot administer the cluster, own application
objects, inherit broader roles, or bypass RLS. Give each named Plaid connection
its own database role when rules require different privileges.

PostgreSQL grants `CONNECT` and `TEMPORARY` on databases, and `EXECUTE` on
functions, to `PUBLIC` by default. Revoking a privilege only from
`plaid_reader` does not remove a privilege inherited from `PUBLIC`. On a
dedicated database, a least-privilege setup can start with:

```sql
CREATE ROLE plaid_reader
    LOGIN
    PASSWORD 'replace-through-your-secret-manager'
    NOSUPERUSER
    NOCREATEDB
    NOCREATEROLE
    NOREPLICATION
    NOBYPASSRLS
    NOINHERIT;

ALTER ROLE plaid_reader SET default_transaction_read_only = on;
ALTER ROLE plaid_reader IN DATABASE application SET search_path = pg_catalog;

REVOKE CONNECT, TEMPORARY ON DATABASE application FROM PUBLIC;
GRANT CONNECT ON DATABASE application TO plaid_reader;

REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA reporting TO plaid_reader;
GRANT SELECT ON reporting.curated_customer_view TO plaid_reader;

REVOKE EXECUTE ON ALL FUNCTIONS IN SCHEMA reporting FROM PUBLIC;
ALTER DEFAULT PRIVILEGES FOR ROLE application_owner
    REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;
```

Revoking database or function privileges from `PUBLIC` affects every role that
relies on those defaults. Coordinate those changes with the database owner in a
shared database, then grant required privileges explicitly. Audit existing
functions before enabling Plaid, grant `EXECUTE` only on functions a rule must
call, and repeat the audit when extensions or application functions change.
`SECURITY DEFINER` functions require particular care.

Curated SQL should use schema-qualified object names. Prefer curated views and
grant no ownership. Enforce row-level security where applicable and verify
policies using the exact login role. The role should not be a member of broader
roles that restore privileges through inheritance or `SET ROLE`.

Production operators should also configure server-side connection, statement,
idle-transaction, and audit-logging policies. Review effective grants and RLS
regularly; Plaid's transaction setting is defense in depth, not a replacement
for database authorization.

Operational query logs contain only connection name, calling module, elapsed
time, row count, a SHA-256 SQL fingerprint, and categorized outcome. SQL,
parameters, query results, passwords, DSNs, CA contents, certificates, and keys
are not logged.
