#!/bin/bash
# Runs inside the PostgreSQL container. Reads password files there.
# The host argv does not contain password bytes.
# Role names stay the frozen live_chain identity. The password file name is a274.
set -eu
umask 077
admin=$(tr -d '\r\n' < /secrets/admin.pass)
reader=$(tr -d '\r\n' < /secrets/reader.pass)
writer=$(tr -d '\r\n' < /secrets/writer.pass)
runtime=$(tr -d '\r\n' < /secrets/runtime.pass)
login=$(tr -d '\r\n' < /secrets/login.pass)
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_fixture_admin "$admin" > /tmp/a274-pgpass
chmod 600 /tmp/a274-pgpass
export PGPASSFILE=/tmp/a274-pgpass
PSQL=(psql -h 127.0.0.1 -U oo_fixture_admin -v ON_ERROR_STOP=1)
"${PSQL[@]}" -d postgres <<SQL
CREATE ROLE oo_a261_reader LOGIN PASSWORD '${reader}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT;
CREATE ROLE oo_a261_writer LOGIN PASSWORD '${writer}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT;
CREATE ROLE oo_a261_runtime LOGIN PASSWORD '${runtime}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT;
CREATE ROLE oo_a261_login LOGIN PASSWORD '${login}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION NOINHERIT;
CREATE DATABASE oo_a261_src TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8';
CREATE DATABASE oo_a261_dst TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8';
GRANT CONNECT ON DATABASE oo_a261_src TO oo_a261_reader, oo_a261_writer;
GRANT CONNECT ON DATABASE oo_a261_dst TO oo_a261_runtime, oo_a261_login;
SQL
"${PSQL[@]}" -d oo_a261_src -f /tmp/source.sql >/dev/null
version=$("${PSQL[@]}" -d postgres -Atqc "SHOW server_version")
autovacuum=$("${PSQL[@]}" -d postgres -Atqc "SHOW autovacuum")
ssl=$("${PSQL[@]}" -d postgres -Atqc "SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
printf 'server_version=%s\nautovacuum=%s\nssl=%s\n' "$version" "$autovacuum" "$ssl"
