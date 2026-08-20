-- Deterministic local fixture data for the PostgreSQL API example.
-- The administrator role is created by the official PostgreSQL image; Plaid
-- connects only as this least-privilege reader.

CREATE ROLE plaid_reader
    LOGIN
    PASSWORD 'plaid_reader'
    NOSUPERUSER
    NOCREATEDB
    NOCREATEROLE
    NOREPLICATION
    NOBYPASSRLS
    NOINHERIT;

ALTER ROLE plaid_reader SET default_transaction_read_only = on;
ALTER ROLE plaid_reader IN DATABASE plaid_local SET search_path = pg_catalog;

CREATE SCHEMA demo AUTHORIZATION plaid_admin;

CREATE TABLE demo.people (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    active BOOLEAN NOT NULL,
    profile JSONB NOT NULL
);

INSERT INTO demo.people (id, name, active, profile) VALUES
    (1, 'Ada Lovelace', TRUE, '{"team": "security"}'),
    (2, 'Grace Hopper', FALSE, '{"team": "platform"}'),
    (3, 'Linus Torvalds', TRUE, '{"team": "infrastructure"}');

REVOKE CONNECT, TEMPORARY ON DATABASE plaid_local FROM PUBLIC;
GRANT CONNECT ON DATABASE plaid_local TO plaid_reader;
GRANT USAGE ON SCHEMA demo TO plaid_reader;
GRANT SELECT ON TABLE demo.people TO plaid_reader;
