#!/bin/bash
# Runs inside the official PostgreSQL entrypoint, before statement logging is enabled.
# Passwords are read from files. They are not arguments and they are not echoed.
set -euo pipefail

sql_quote() {
  printf "%s" "$1" | sed "s/'/''/g"
}

require_ident() {
  case "$1" in
    "" | *[!a-z0-9_]*) echo "init: ident" >&2; exit 1 ;;
  esac
  case "$1" in
    [a-z_]*) ;;
    *) echo "init: ident" >&2; exit 1 ;;
  esac
}

mode="${OO_MODE:?init: mode}"
database="${OO_DATABASE:?init: database}"
require_ident "$database"

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname postgres >/dev/null <<SQL
ALTER SYSTEM SET ssl = on;
ALTER SYSTEM SET ssl_cert_file = '/var/lib/postgresql/server.crt';
ALTER SYSTEM SET ssl_key_file = '/var/lib/postgresql/server.key';
ALTER SYSTEM SET ssl_min_protocol_version = 'TLSv1.3';
ALTER SYSTEM SET log_statement = 'all';
ALTER SYSTEM SET logging_collector = on;
ALTER SYSTEM SET log_destination = 'csvlog';
ALTER SYSTEM SET log_connections = on;
ALTER SYSTEM SET log_disconnections = on;
SQL

if [[ "$mode" == "source" ]]; then
  dba="${OO_DBA:?init: dba}"
  reader="${OO_READER:?init: reader}"
  denied="${OO_DENIED:?init: denied}"
  require_ident "$dba"
  require_ident "$reader"
  require_ident "$denied"
  dba_pw="$(sql_quote "$(< /run/secrets/dba)")"
  reader_pw="$(sql_quote "$(< /run/secrets/reader)")"
  denied_pw="$(sql_quote "$(< /run/secrets/denied)")"
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname postgres >/dev/null <<SQL
CREATE ROLE ${dba} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD '${dba_pw}';
CREATE ROLE ${reader} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD '${reader_pw}';
CREATE ROLE ${denied} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD '${denied_pw}';
CREATE DATABASE ${database} OWNER ${dba};
REVOKE ALL ON DATABASE ${database} FROM PUBLIC;
GRANT CONNECT ON DATABASE ${database} TO ${reader};
GRANT CONNECT ON DATABASE ${database} TO ${denied};
SQL
elif [[ "$mode" == "target" ]]; then
  owner="${OO_TARGET_USER:?init: target}"
  require_ident "$owner"
  target_pw="$(sql_quote "$(< /run/secrets/target)")"
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname postgres >/dev/null <<SQL
CREATE ROLE ${owner} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD '${target_pw}';
CREATE DATABASE ${database} OWNER ${owner};
GRANT CONNECT ON DATABASE ${database} TO ${owner};
SQL
else
  echo "init: mode" >&2
  exit 1
fi
