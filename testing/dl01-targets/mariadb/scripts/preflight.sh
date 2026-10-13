#!/bin/bash
# Native client and server preflight. Does not compile Rust.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"
mkdir -p "$OO_EVIDENCE/raw"

echo "msrv_recipe=testing/infra/builders/container-cargo.sh"
echo "msrv_image=docker.io/library/rust:1.94.0"
echo "msrv_applicability=pending_until_rust_consumer"
echo "rust_compile=no"
echo "mysql_equivalence=not_used"

arch=$(docker image inspect "$OO_IMAGE_DIGEST" --format '{{.Architecture}}')
repo=$(docker image inspect "$OO_IMAGE_DIGEST" --format '{{json .RepoDigests}}')
echo "image_arch=$arch"
echo "image_repo_digests=$repo"
[ "$arch" = "arm64" ]
printf '%s' "$repo" | grep -q "0130d92c05fbf2d82adc2b86de742eaede65b2596c65d91786f8e03cd19e6a39"

client=$(docker exec "$OO_CONTAINER" mariadb --version)
echo "client=$client"
printf '%s\n' "$client" | grep -q 'MariaDB'
printf '%s\n' "$client" | grep -q '11.4.13'

oo_reader <<'SQL' > "$OO_EVIDENCE/raw/preflight-reader.txt"
SELECT CONCAT('version|', VERSION());
SELECT CONCAT('comment|', @@version_comment);
SELECT CONCAT('log_bin|', @@log_bin + 0);
SELECT CONCAT('time_zone|', @@global.time_zone);
SELECT CONCAT('collation_server|', @@collation_server);
SELECT CONCAT('character_set_server|', @@character_set_server);
SELECT CONCAT('sql_mode|', @@sql_mode);
SELECT CONCAT('schema_collation|', DEFAULT_COLLATION_NAME)
FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = 'oo_synth';
SELECT CONCAT('json_data_type|', DATA_TYPE)
FROM information_schema.COLUMNS
WHERE TABLE_SCHEMA = 'oo_synth' AND TABLE_NAME = 'k_json' AND COLUMN_NAME = 'doc';
SELECT CONCAT('json_column_type|', COLUMN_TYPE)
FROM information_schema.COLUMNS
WHERE TABLE_SCHEMA = 'oo_synth' AND TABLE_NAME = 'k_json' AND COLUMN_NAME = 'doc';
SELECT CONCAT('gtid_current_pos|', @@gtid_current_pos);
SELECT CONCAT('gtid_domain_id|', @@gtid_domain_id);
SQL

field() {
  awk -F'|' -v key="$1" '$1 == key { print substr($0, length(key) + 2) }' "$OO_EVIDENCE/raw/preflight-reader.txt"
}

version=$(field version)
comment=$(field comment)
log_bin=$(field log_bin)
schema_collation=$(field schema_collation)
json_data_type=$(field json_data_type)
echo "version=$version"
echo "comment=$comment"
echo "log_bin=$log_bin"
echo "schema_collation=$schema_collation"
echo "json_data_type=$json_data_type"
printf '%s\n' "$version" | grep -q '11\.4\.13-MariaDB'
printf '%s\n' "$comment" | grep -qi 'mariadb'
[ "$log_bin" = "0" ]
[ "$schema_collation" = "utf8mb4_bin" ]
[ "$(printf '%s' "$json_data_type" | tr '[:upper:]' '[:lower:]')" = "longtext" ]
sql_mode=$(field sql_mode)
printf '%s\n' "$sql_mode" | grep -q 'NO_ZERO_DATE'
printf '%s\n' "$sql_mode" | grep -q 'NO_ZERO_IN_DATE'

raw=$(mktemp)
trap 'rm -f "$raw"' EXIT
{
  echo "SHOW GRANTS FOR 'oo_reader'@'localhost';"
  echo "SHOW GRANTS FOR 'oo_reader'@'%';"
  echo "SHOW GRANTS FOR 'oo_dba'@'localhost';"
  echo "SHOW GRANTS FOR 'oo_dba'@'%';"
} | oo_root > "$raw"
python3 "$OO_ROOT/scripts/grants_check.py" "$raw" \
  > "$OO_EVIDENCE/raw/grants.redacted.txt" \
  2> "$OO_EVIDENCE/raw/grants.stderr"
rm -f "$raw"
trap - EXIT
python3 "$OO_ROOT/scripts/secret_scan.py" "$OO_SECRET_DIR" "$OO_EVIDENCE/raw/grants.redacted.txt"
echo "preflight=ok"
