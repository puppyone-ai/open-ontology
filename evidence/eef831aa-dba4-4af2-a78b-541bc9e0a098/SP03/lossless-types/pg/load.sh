#!/bin/bash
# Admin seed. The reader password is read from a file and is not an argument.
# Statement logging is off until the role exists.
set -eu
gosu postgres psql -v ON_ERROR_STOP=1 -d postgres <<SQL
CREATE ROLE oo_type_reader LOGIN PASSWORD '$(cat /secrets/reader.pass)' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOINHERIT;
CREATE DATABASE typedb OWNER postgres;
REVOKE ALL ON DATABASE typedb FROM PUBLIC;
GRANT CONNECT ON DATABASE typedb TO oo_type_reader;
SQL
gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -c "ALTER SYSTEM SET log_statement = 'all'"
gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -c "SELECT pg_reload_conf()"
gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -f /sql/seed.sql
