#!/bin/bash
# Runs inside the MariaDB container. Reads password files and does not print them.
set -euo pipefail
umask 077

hex_file() {
  local value
  value=$(tr -d '\n\r' < "$1")
  if ! printf '%s' "$value" | grep -Eq '^[0-9a-f]{48}$'; then
    echo "refusing secret that is not 48 hex characters: $(basename "$1")" >&2
    exit 1
  fi
  printf '%s' "$value"
}

dba=$(hex_file /run/secrets/dba.pw)
reader=$(hex_file /run/secrets/reader.pw)
sql=/tmp/oo-create-users.sql
trap 'rm -f "$sql"; unset dba reader' EXIT
{
  echo "CREATE DATABASE IF NOT EXISTS oo_synth CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;"
  printf "CREATE USER IF NOT EXISTS 'w39_dba'@'localhost' IDENTIFIED BY '%s';\n" "$dba"
  printf "CREATE USER IF NOT EXISTS 'w39_dba'@'%%' IDENTIFIED BY '%s';\n" "$dba"
  printf "CREATE USER IF NOT EXISTS 'w39_reader'@'localhost' IDENTIFIED BY '%s';\n" "$reader"
  printf "CREATE USER IF NOT EXISTS 'w39_reader'@'%%' IDENTIFIED BY '%s';\n" "$reader"
} > "$sql"
chmod 600 "$sql"
unset dba reader
mariadb --defaults-extra-file=/run/secrets/root.cnf --protocol=socket --socket=/run/mysqld/mysqld.sock --batch --silent < "$sql"
rm -f "$sql"
trap - EXIT
echo "bootstrap_users=created"
