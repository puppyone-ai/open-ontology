#!/bin/bash
# Runs inside the PostgreSQL container. Reads password files there.
# The host argv does not contain password bytes. OO_DEMO_OWNER is a four-hex token.
set -eu
umask 077
owner=${OO_DEMO_OWNER:-}
case "$owner" in
  [a-f0-9][a-f0-9][a-f0-9][a-f0-9]) ;;
  *)
    echo "owner token refused" >&2
    exit 2
    ;;
esac
admin=$(tr -d '\r\n' < /secrets/admin.pass)
reader=$(tr -d '\r\n' < /secrets/reader.pass)
writer=$(tr -d '\r\n' < /secrets/writer.pass)
runtime=$(tr -d '\r\n' < /secrets/runtime.pass)
login=$(tr -d '\r\n' < /secrets/login.pass)
reader_role="oo_${owner}_reader"
writer_role="oo_${owner}_writer"
runtime_role="oo_${owner}_runtime"
login_role="oo_${owner}_login"
db_src="oo_${owner}_src"
db_dst="oo_${owner}_dst"
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_fixture_admin "$admin" > /tmp/demo-pgpass
chmod 600 /tmp/demo-pgpass
export PGPASSFILE=/tmp/demo-pgpass
PSQL=(psql -h 127.0.0.1 -U oo_fixture_admin -v ON_ERROR_STOP=1)
"${PSQL[@]}" -d postgres <<SQL
CREATE ROLE ${reader_role} LOGIN PASSWORD '${reader}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT;
CREATE ROLE ${writer_role} LOGIN PASSWORD '${writer}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT;
CREATE ROLE ${runtime_role} LOGIN PASSWORD '${runtime}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT;
CREATE ROLE ${login_role} LOGIN PASSWORD '${login}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION NOINHERIT;
CREATE DATABASE ${db_src} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8';
CREATE DATABASE ${db_dst} TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8';
GRANT CONNECT ON DATABASE ${db_src} TO ${reader_role}, ${writer_role};
GRANT CONNECT ON DATABASE ${db_dst} TO ${runtime_role}, ${login_role};
SQL
"${PSQL[@]}" -v reader="$reader_role" -v writer="$writer_role" -d "$db_src" -f /tmp/source.sql >/dev/null
version=$("${PSQL[@]}" -d postgres -Atqc "SHOW server_version")
autovacuum=$("${PSQL[@]}" -d postgres -Atqc "SHOW autovacuum")
ssl=$("${PSQL[@]}" -d postgres -Atqc "SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
printf 'server_version=%s\nautovacuum=%s\nssl=%s\n' "$version" "$autovacuum" "$ssl"
