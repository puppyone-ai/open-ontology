#!/bin/bash
# Runs inside the PostgreSQL container. Password bytes stay in this process.
# Host argv contains paths only.
set -eu
umask 077
admin=$(tr -d '\r\n' < /secrets/admin.pass)
reader=$(tr -d '\r\n' < /secrets/reader.pass)
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_a328_admin "$admin" > /tmp/a328-pgpass
chmod 600 /tmp/a328-pgpass
export PGPASSFILE=/tmp/a328-pgpass
PSQL=(psql -h 127.0.0.1 -U oo_a328_admin -v ON_ERROR_STOP=1)
"${PSQL[@]}" -d postgres <<SQL
CREATE ROLE oo_a328_reader LOGIN PASSWORD '${reader}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT;
CREATE DATABASE oo_a328_src TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8';
GRANT CONNECT ON DATABASE oo_a328_src TO oo_a328_reader;
SQL
"${PSQL[@]}" -d oo_a328_src -f /tmp/a328-schema.sql >/dev/null
"${PSQL[@]}" -d oo_a328_src <<'SQL'
\copy shop.party FROM '/tmp/a328-party.copy' WITH (FORMAT text)
\copy shop.account FROM '/tmp/a328-account.copy' WITH (FORMAT text)
\copy shop.entry FROM '/tmp/a328-entry.copy' WITH (FORMAT text)
ANALYZE shop.party;
ANALYZE shop.account;
ANALYZE shop.entry;
SQL
"${PSQL[@]}" -d oo_a328_src -Atqc "SELECT table_name || ' ' || grantee || ' ' || privilege_type FROM information_schema.role_table_grants WHERE table_schema = 'shop' ORDER BY 1" > /tmp/a328-grants.txt
"${PSQL[@]}" -d oo_a328_src -Atqc "SELECT relname || '=' || count FROM (SELECT 'party' AS relname, count(*) FROM shop.party UNION ALL SELECT 'account', count(*) FROM shop.account UNION ALL SELECT 'entry', count(*) FROM shop.entry) AS counts ORDER BY 1" > /tmp/a328-counts.txt
"${PSQL[@]}" -d oo_a328_src -Atqc "SELECT c.relname || '=' || c.reltuples::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'shop' AND c.relkind = 'r' ORDER BY 1" > /tmp/a328-reltuples.txt
version=$("${PSQL[@]}" -d postgres -Atqc "SHOW server_version")
autovacuum=$("${PSQL[@]}" -d postgres -Atqc "SHOW autovacuum")
ssl=$("${PSQL[@]}" -d postgres -Atqc "SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
printf 'server_version=%s\nautovacuum=%s\nssl=%s\n' "$version" "$autovacuum" "$ssl"
