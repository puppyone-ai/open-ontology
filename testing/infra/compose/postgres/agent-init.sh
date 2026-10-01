#!/bin/bash
# First-boot roles and synthetic tables for the local Agent DB fixture.
# Passwords are 48 hex characters from the secret files. They are not echoed.
set -euo pipefail

hex_file() {
  local value
  value=$(tr -d '\n' < "$1")
  if ! printf '%s' "$value" | grep -Eq '^[0-9a-f]{48}$'; then
    echo "refusing non-hex postgres secret $(basename "$1")" >&2
    exit 1
  fi
  printf '%s' "$value"
}

repl=$(hex_file /run/secrets/replication_password)
reader=$(hex_file /run/secrets/agent_ro_password)
umask 077
sql=/tmp/oo-agent-init.sql
trap 'rm -f "$sql"' EXIT
{
  printf "CREATE ROLE oo_replicator WITH REPLICATION LOGIN PASSWORD '%s';\n" "$repl"
  printf "CREATE ROLE oo_agent_reader WITH LOGIN PASSWORD '%s';\n" "$reader"
  cat <<'SQL'
CREATE SCHEMA oo_fixture;
CREATE TABLE oo_fixture.ownership (
    id integer PRIMARY KEY,
    fixture text NOT NULL,
    note text NOT NULL
);
INSERT INTO oo_fixture.ownership (id, fixture, note)
VALUES (1, 'pg-agent', 'synthetic local agent fixture');
CREATE TABLE oo_fixture.replay_probe (
    id integer PRIMARY KEY,
    note text NOT NULL
);
REVOKE ALL ON SCHEMA oo_fixture FROM PUBLIC;
GRANT USAGE ON SCHEMA oo_fixture TO oo_agent_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA oo_fixture TO oo_agent_reader;
ALTER DEFAULT PRIVILEGES IN SCHEMA oo_fixture
    GRANT SELECT ON TABLES TO oo_agent_reader;
SQL
} > "$sql"
chmod 600 "$sql"
unset repl reader
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" -f "$sql"
rm -f "$sql"
printf 'host replication oo_replicator 0.0.0.0/0 scram-sha-256\n' >> "$PGDATA/pg_hba.conf"
printf 'host replication oo_replicator ::/0 scram-sha-256\n' >> "$PGDATA/pg_hba.conf"
echo "agent_roles_bootstrapped=yes"
