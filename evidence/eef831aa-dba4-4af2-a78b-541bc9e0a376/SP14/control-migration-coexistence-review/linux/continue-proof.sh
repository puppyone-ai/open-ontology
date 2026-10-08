#!/bin/bash
# Resume only the phases run-proof.sh did not finish.
# The live containers are oo-a376-proof and oo-a376-pg. This script does not
# create a container, does not pull, and does not repeat a green cargo test.
# Attempt 1 omitted docker exec -i, so psql saw EOF, raw-create.log.exit is 0
# with an empty log, and facts-deny-oo_a365_*.log are empty. Those files stay.
# Retry logs use the -retry suffix. facts-deny.txt and facts-raw.txt are the
# oracle inputs filled from the retry.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a376
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/control-migration-coexistence-review
PROOF=${EVIDENCE}/proof
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
PG_NAME=oo-a376-pg
BNAME=oo-a376-proof
CAL_SQL=${REPO}/tools/calendar-store/migrations/oo_calendar_control_v1.sql
ENUM_SQL=${REPO}/tools/enum-store/migrations/oo_enum_control_v1.sql

redact() { sed -e 's/[0-9a-f]\{48\}/redacted/g' -e "s/PASSWORD '[^']*'/PASSWORD 'redacted'/g"; }
mark() { printf '%s %s\n' "$(date -u +%H:%M:%S)" "$1" | tee -a "$PROOF/progress.txt"; }

if [ -f "$PROOF/proof.exit" ]; then
  echo "proof.exit already recorded" >&2
  exit 2
fi
proof_img=$(docker inspect "$BNAME" --format '{{.Image}}')
pg_img=$(docker inspect "$PG_NAME" --format '{{.Image}}')
want_rust=$(docker image inspect "$RUST_IMAGE" --format '{{.Id}}')
want_pg=$(docker image inspect "$PG_IMAGE" --format '{{.Id}}')
[ "$proof_img" = "$want_rust" ]
[ "$pg_img" = "$want_pg" ]
docker inspect "$BNAME" --format '{{.State.Running}} {{index .Config.Labels "oo.owner"}} {{index .Config.Labels "oo.session"}}' | grep -qx "true a376 ${RUN_ID}"
docker inspect "$PG_NAME" --format '{{.State.Running}} {{index .Config.Labels "oo.owner"}} {{index .Config.Labels "oo.session"}}' | grep -qx "true a376 ${RUN_ID}"
running_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
own_builders=$(docker ps -q --filter label=oo.owner=a376 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_builders=%s global_builders=%s\n' "$own_builders" "$running_builders" > "$PROOF/continue-capacity.txt"
[ "$own_builders" = 1 ]
[ "$running_builders" -le 10 ]
docker exec -u postgres "$PG_NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null
mark continue-start

psql_q() {
  local db=$1
  local sql=$2
  docker exec -u postgres "$PG_NAME" psql -v ON_ERROR_STOP=1 -q -A -t -F '|' --no-psqlrc \
    -U oo_fixture_admin -d "$db" -c "$sql"
}

psql_in() {
  local db=$1
  local log=$2
  shift 2
  set +e
  docker exec -u postgres -i "$PG_NAME" psql -v ON_ERROR_STOP=1 -q --no-psqlrc \
    -U oo_fixture_admin -d "$db" "$@" > "$log" 2>&1
  local code=$?
  set -e
  redact < "$log" > "${log}.redacted"
  mv "${log}.redacted" "$log"
  printf '%s\n' "$code"
}

capture_deny() {
  local db=$1
  local role=$2
  local schema=$3
  local log=$PROOF/facts-deny-retry-${db}.log
  local code
  code=$(psql_in "$db" "$log" <<SQL
BEGIN;
DO \$body\$
BEGIN
  BEGIN
    EXECUTE 'SET LOCAL ROLE ${role}';
    PERFORM ${schema}.dict_create_open();
    RAISE EXCEPTION 'succeeded';
  EXCEPTION
    WHEN insufficient_privilege THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|open|42501';
      RAISE NOTICE '%', 'denymsg|${db}|${role}|open|' || SQLERRM;
    WHEN OTHERS THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|open|' || SQLSTATE;
      RAISE NOTICE '%', 'denymsg|${db}|${role}|open|' || SQLERRM;
  END;
  BEGIN
    EXECUTE 'SET LOCAL ROLE ${role}';
    PERFORM ${schema}.dict_grant_runtime('oo_r_other');
    RAISE EXCEPTION 'succeeded';
  EXCEPTION
    WHEN insufficient_privilege THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|grant|42501';
      RAISE NOTICE '%', 'denymsg|${db}|${role}|grant|' || SQLERRM;
    WHEN OTHERS THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|grant|' || SQLSTATE;
      RAISE NOTICE '%', 'denymsg|${db}|${role}|grant|' || SQLERRM;
  END;
  BEGIN
    EXECUTE 'SET LOCAL ROLE ${role}';
    EXECUTE 'CREATE TABLE oo_dict.a376_no (n integer)';
    RAISE EXCEPTION 'succeeded';
  EXCEPTION
    WHEN insufficient_privilege THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|create|42501';
      RAISE NOTICE '%', 'denymsg|${db}|${role}|create|' || SQLERRM;
    WHEN OTHERS THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|create|' || SQLSTATE;
      RAISE NOTICE '%', 'denymsg|${db}|${role}|create|' || SQLERRM;
  END;
END
\$body\$;
ROLLBACK;
SQL
)
  printf '%s\n' "$code" > "${log}.exit"
  sed -n 's/^NOTICE:[[:space:]]*//p' "$log" >> "$PROOF/facts-deny.txt"
}

if [ ! -s "$PROOF/facts-deny.txt" ]; then
  mark deny-retry
  : > "$PROOF/facts-deny.txt"
  capture_deny oo_a365_cal_first oo_r_enum oo_calendar_control
  capture_deny oo_a365_enum_first oo_r_calendar oo_enum_control
  psql_q oo_a365_cal_first "SELECT 'deny_table', current_database(), count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_dict' AND c.relname = 'a376_no'" >> "$PROOF/facts-deny.txt"
  psql_q oo_a365_enum_first "SELECT 'deny_table', current_database(), count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_dict' AND c.relname = 'a376_no'" >> "$PROOF/facts-deny.txt"
fi

have_raw=$(psql_q postgres "SELECT count(*) FROM pg_catalog.pg_database WHERE datname IN ('oo_a376_raw_tx','oo_a376_raw_auto')" | tr -d '[:space:]')
if [ "$have_raw" != 2 ]; then
  mark raw-create-retry
  code=$(psql_in postgres "$PROOF/raw-create-retry.log" <<'SQL'
CREATE DATABASE oo_a376_raw_tx OWNER oo_fixture_admin;
CREATE DATABASE oo_a376_raw_auto OWNER oo_fixture_admin;
REVOKE ALL ON DATABASE oo_a376_raw_tx FROM PUBLIC;
REVOKE ALL ON DATABASE oo_a376_raw_auto FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE oo_a376_raw_tx TO oo_calendar_admin, oo_enum_admin;
GRANT CONNECT, CREATE ON DATABASE oo_a376_raw_auto TO oo_calendar_admin, oo_enum_admin;
SQL
)
  printf '%s\n' "$code" > "$PROOF/raw-create-retry.log.exit"
  if [ "$code" != 0 ]; then
    echo "raw database create failed" >&2
    exit "$code"
  fi
fi

run_sql_file() {
  local db=$1
  local role=$2
  local file=$3
  local log=$4
  shift 4
  set +e
  {
    printf '%s\n' '\set VERBOSITY verbose'
    printf '%s\n' "SET ROLE ${role};"
    cat "$file"
  } | docker exec -u postgres -i "$PG_NAME" psql -v ON_ERROR_STOP=1 -q --no-psqlrc \
    -U oo_fixture_admin -d "$db" "$@" > "$log" 2>&1
  local code=$?
  set -e
  redact < "$log" > "${log}.redacted"
  mv "${log}.redacted" "$log"
  printf '%s\n' "$code" > "${log}.exit"
  printf '%s\n' "$code"
}

if [ ! -f "$PROOF/raw-tx-calendar-retry.log.exit" ]; then
  mark raw-tx-retry
  run_sql_file oo_a376_raw_tx oo_calendar_admin "$CAL_SQL" "$PROOF/raw-tx-calendar-retry.log" --single-transaction >/dev/null
  run_sql_file oo_a376_raw_tx oo_enum_admin "$ENUM_SQL" "$PROOF/raw-tx-enum-retry.log" --single-transaction --variable=VERBOSITY=verbose >/dev/null
fi
if [ ! -f "$PROOF/raw-auto-calendar-retry.log.exit" ]; then
  mark raw-auto-retry
  run_sql_file oo_a376_raw_auto oo_calendar_admin "$CAL_SQL" "$PROOF/raw-auto-calendar-retry.log" >/dev/null
  run_sql_file oo_a376_raw_auto oo_enum_admin "$ENUM_SQL" "$PROOF/raw-auto-enum-retry.log" --variable=VERBOSITY=verbose >/dev/null
fi

if [ ! -f "$PROOF/facts-raw.txt" ]; then
  mark facts-raw
  : > "$PROOF/facts-raw.txt"
  sqlstate=$(sed -n 's/.*SQLSTATE: //p' "$PROOF/raw-tx-enum-retry.log" | head -1 | tr -d '[:space:]')
  printf '%s\n' "raw_tx|second|${sqlstate}" >> "$PROOF/facts-raw.txt"
  psql_q oo_a376_raw_tx "SELECT 'raw_tx|oo_calendar_control', count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_calendar_control'" >> "$PROOF/facts-raw.txt"
  psql_q oo_a376_raw_tx "SELECT 'raw_tx|oo_enum_control', count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_enum_control'" >> "$PROOF/facts-raw.txt"
  psql_q oo_a376_raw_tx "SELECT 'raw_tx|oo_dict', count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_dict'" >> "$PROOF/facts-raw.txt"
  psql_q oo_a376_raw_auto "SELECT 'raw_auto|oo_enum_control', count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_enum_control'" >> "$PROOF/facts-raw.txt"
  psql_q oo_a376_raw_auto "SELECT 'raw_auto|oo_calendar_control', count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_calendar_control'" >> "$PROOF/facts-raw.txt"
  psql_q oo_a376_raw_auto "SELECT 'raw_auto|oo_dict', count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_dict'" >> "$PROOF/facts-raw.txt"
  psql_q oo_a376_raw_auto "SELECT 'raw_auto|dual', count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_dict' AND c.relname IN ('calendar','enums') AND c.relkind = 'v'" >> "$PROOF/facts-raw.txt"
  printf '%s\n' "raw_auto|second_exit|$(tr -d '[:space:]' < "$PROOF/raw-auto-enum-retry.log.exit")" >> "$PROOF/facts-raw.txt"
  printf '%s\n' "raw_auto|first_exit|$(tr -d '[:space:]' < "$PROOF/raw-auto-calendar-retry.log.exit")" >> "$PROOF/facts-raw.txt"
  printf '%s\n' "raw_tx|first_exit|$(tr -d '[:space:]' < "$PROOF/raw-tx-calendar-retry.log.exit")" >> "$PROOF/facts-raw.txt"
fi

run_rust() {
  local log=$1
  if [ -f "${log}.exit" ] && [ "$(cat "${log}.exit")" = 0 ]; then
    printf '%s %s\n' "$(date -u +%H:%M:%S)" "kept $(basename "$log")" | tee -a "$PROOF/progress.txt" >&2
    printf '%s\n' 0
    return 0
  fi
  set +e
  docker exec \
    -e HOME=/tmp \
    -e CARGO_HOME=/cargo-home \
    -e CARGO_TARGET_DIR=/target \
    -e CARGO_BUILD_JOBS=1 \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    -e CARGO_TERM_COLOR=never \
    "$BNAME" bash -c "$CMD" > "$log" 2>&1
  local code=$?
  set -e
  printf '%s\n' "$code" > "${log}.exit"
  printf '%s\n' "$code"
}

CMD='cd /src/tools/calendar-store && cargo clippy --locked --offline --all-targets -- -D warnings'
mark clippy-calendar
cal_clippy=$(run_rust "$PROOF/calendar-clippy.log")
CMD='cd /src/tools/enum-store && cargo clippy --locked --offline --all-targets -- -D warnings'
mark clippy-enum
enum_clippy=$(run_rust "$PROOF/enum-clippy.log")

mark oracle
cat "$PROOF/facts-before.txt" "$PROOF/facts-green.txt" "$PROOF/facts-deny.txt" "$PROOF/facts-pathdrift.txt" "$PROOF/facts-refuse.txt" "$PROOF/facts-raw.txt" > "$PROOF/facts.txt"
CMD='rustc --edition 2021 -C opt-level=2 -o /target/a376-oracle /evidence/linux/oracle/main.rs && /target/a376-oracle /evidence/proof/facts.txt /src/tools/calendar-store/migrations/oo_calendar_control_v1.sql /src/tools/enum-store/migrations/oo_enum_control_v1.sql /src/tools/calendar-store/CONTRACT.md /src/tools/enum-store/CONTRACT.md'
oracle_code=$(run_rust "$PROOF/oracle.log")
printf '%s\n' "$oracle_code" > "$PROOF/proof.exit"
mark "continue-done oracle=${oracle_code} clippy=${cal_clippy}/${enum_clippy}"
if [ "$oracle_code" != 0 ] || [ "$cal_clippy" != 0 ] || [ "$enum_clippy" != 0 ]; then
  exit 1
fi
echo continue_ok
