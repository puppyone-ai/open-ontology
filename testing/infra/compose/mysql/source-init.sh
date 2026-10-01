#!/bin/bash
# Synthetic local source objects and a SELECT-only account.
# general_log is off until the password statements have finished.
set -euo pipefail

hex_file() {
  local value
  value=$(tr -d '\n' < "$1")
  if ! printf '%s' "$value" | grep -Eq '^[0-9a-f]{48}$'; then
    echo "refusing non-hex mysql secret $(basename "$1")" >&2
    exit 1
  fi
  printf '%s' "$value"
}

reader=$(hex_file /run/secrets/mysql_reader_password)
root_pw=$(hex_file /run/secrets/mysql_root_password)
umask 077
cnf=/tmp/oo-mysql-client.cnf
sql=/tmp/oo-mysql-init.sql
gl=/tmp/oo-mysql-general.txt
trap 'rm -f "$cnf" "$sql" "$gl"' EXIT
{
  echo '[client]'
  echo 'user=root'
  echo "password=${root_pw}"
  echo 'protocol=socket'
} > "$cnf"
chmod 600 "$cnf"
unset root_pw

{
  cat <<'SQL'
CREATE TABLE IF NOT EXISTS oo_local_test.shipment_leg (
    leg_id BIGINT NOT NULL PRIMARY KEY,
    synthetic_ref CHAR(16) NOT NULL,
    qty INT NOT NULL
);
INSERT IGNORE INTO oo_local_test.shipment_leg (leg_id, synthetic_ref, qty) VALUES
    (1, 'LOCALTEST00000001', 3),
    (2, 'LOCALTEST00000002', 1);
CREATE DATABASE IF NOT EXISTS oo_src_audit;
CREATE TABLE IF NOT EXISTS oo_src_audit.dba_inventory (
    object_name varchar(128) NOT NULL PRIMARY KEY,
    object_kind varchar(32) NOT NULL,
    note varchar(255) NOT NULL
);
INSERT IGNORE INTO oo_src_audit.dba_inventory (object_name, object_kind, note) VALUES
    ('oo_local_test', 'database', 'synthetic local test source'),
    ('oo_local_test.shipment_leg', 'table', 'synthetic rows only'),
    ('oo_src_reader', 'account', 'SELECT on oo_local_test only'),
    ('mysql.general_log', 'audit', 'enabled after auth bootstrap');
SQL
  printf "CREATE USER IF NOT EXISTS 'oo_src_reader'@'%%' IDENTIFIED BY '%s';\n" "$reader"
  printf "CREATE USER IF NOT EXISTS 'oo_src_reader'@'localhost' IDENTIFIED BY '%s';\n" "$reader"
  cat <<'SQL'
GRANT SELECT ON oo_local_test.* TO 'oo_src_reader'@'%';
GRANT SELECT ON oo_local_test.* TO 'oo_src_reader'@'localhost';
CREATE TABLE IF NOT EXISTS oo_src_audit.privilege_snapshot (
    grantee varchar(128) NOT NULL,
    privilege_type varchar(64) NOT NULL,
    object_schema varchar(128) NULL,
    object_name varchar(128) NULL
);
DELETE FROM oo_src_audit.privilege_snapshot;
INSERT INTO oo_src_audit.privilege_snapshot (grantee, privilege_type, object_schema, object_name)
SELECT GRANTEE, PRIVILEGE_TYPE, TABLE_SCHEMA, NULL
FROM information_schema.SCHEMA_PRIVILEGES
WHERE GRANTEE LIKE '%oo_src_reader%'
UNION ALL
SELECT GRANTEE, PRIVILEGE_TYPE, TABLE_SCHEMA, TABLE_NAME
FROM information_schema.TABLE_PRIVILEGES
WHERE GRANTEE LIKE '%oo_src_reader%';
SQL
} > "$sql"
chmod 600 "$sql"
mysql --defaults-extra-file="$cnf" < "$sql"
rm -f "$sql"
unset reader

mysql --defaults-extra-file="$cnf" -N -e "SELECT COALESCE(argument,'') FROM mysql.general_log" > "$gl"
hits=0
for secret in /run/secrets/mysql_reader_password /run/secrets/mysql_root_password; do
  count=$(grep -F -c -f "$secret" "$gl" || true)
  hits=$((hits + count))
done
identified=$(mysql --defaults-extra-file="$cnf" -N -e "SELECT COUNT(*) FROM mysql.general_log WHERE argument LIKE '%IDENTIFIED BY%'")
echo "bootstrap_general_log_secret_hits=$hits"
echo "bootstrap_identified_by_rows=$identified"
if [ "$hits" -ne 0 ] || [ "$identified" -ne 0 ]; then
  echo "mysql bootstrap wrote a generated secret into general_log; leaving the data volume" >&2
  exit 1
fi
rm -f "$gl"

mysql --defaults-extra-file="$cnf" <<'SQL'
SET PERSIST general_log = ON;
SET PERSIST log_output = 'TABLE';
SET GLOBAL general_log = ON;
SET GLOBAL log_output = 'TABLE';
SELECT 'oo_audit_log_enabled';
SQL
on=$(mysql --defaults-extra-file="$cnf" -N -e "SELECT @@global.general_log")
out=$(mysql --defaults-extra-file="$cnf" -N -e "SELECT @@global.log_output")
echo "general_log_after_bootstrap=$on"
echo "log_output_after_bootstrap=$out"
[ "$on" = "1" ]
[ "$out" = "TABLE" ]
