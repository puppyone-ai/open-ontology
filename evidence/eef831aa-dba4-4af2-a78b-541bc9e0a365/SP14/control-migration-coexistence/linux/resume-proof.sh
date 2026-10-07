#!/bin/bash
# Resume on the existing a365 builder and postgres. Do not start another of either.
# Do not rerun fetch or the lib/contract phases that already exited 0.
# Sync the repaired installer sources into the archive the builder already mounts.
# This script already ran through enum-store solo_reject (exit 101).
# A later resume that finds calendar solo exit 0 must use resume-proof-2.sh.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a365
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/control-migration-coexistence
PROOF=${EVIDENCE}/proof
PG_NAME=oo-a365-pg
BNAME=oo-a365-proof
TMP=/tmp/oo-a365-eef831aa-dba4-4af2-a78b-541bc9e0a365
SEC=${TMP}/secrets
FIX=${TMP}/fixture
ARCH=${TMP}/archive

redact() { sed -e 's/\b[0-9a-f]\{48\}\b/redacted/g' -e "s/PASSWORD '[^']*'/PASSWORD 'redacted'/g"; }

owner=$(docker inspect "$PG_NAME" --format '{{index .Config.Labels "oo.owner"}} {{index .Config.Labels "oo.session"}} {{index .Config.Labels "oo.kind"}}')
bowner=$(docker inspect "$BNAME" --format '{{index .Config.Labels "oo.owner"}} {{index .Config.Labels "oo.session"}} {{index .Config.Labels "oo.kind"}}')
if [ "$owner" != "a365 ${RUN_ID} postgres" ] || [ "$bowner" != "a365 ${RUN_ID} rust-builder" ]; then
  echo "refusing to reuse an unexpected container" >&2
  exit 2
fi
if ! docker exec -u postgres "$PG_NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
  echo "existing postgres is not ready" >&2
  exit 1
fi
for rel in src/sql.rs src/adopt.rs; do
  cp "$REPO/tools/calendar-store/$rel" "$ARCH/tools/calendar-store/$rel"
  cp "$REPO/tools/enum-store/$rel" "$ARCH/tools/enum-store/$rel"
done
cp "$REPO/tools/calendar-store/CONTRACT.md" "$ARCH/tools/calendar-store/CONTRACT.md"
cp "$REPO/tools/enum-store/CONTRACT.md" "$ARCH/tools/enum-store/CONTRACT.md"

run_rust() {
  local log=$1
  shift
  set +e
  docker exec \
    -e HOME=/tmp \
    -e CARGO_HOME=/cargo-home \
    -e CARGO_TARGET_DIR=/target \
    -e CARGO_BUILD_JOBS=1 \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    -e CARGO_TERM_COLOR=never \
    "$@" \
    "$BNAME" bash -c "$CMD" 2>&1 | redact > "$log"
  local code=${PIPESTATUS[0]}
  set -e
  printf '%s\n' "$code" > "${log}.exit"
  if [ "$code" -ne 0 ]; then
    echo "command failed: $log" >&2
    exit "$code"
  fi
}

set_db() { printf '%s\n' oo-a365-pg 5432 "$1" > "$FIX/endpoint.txt"; }

run_phase() {
  local crate=$1
  local phase=$2
  local db=$3
  set_db "$db"
  CMD="cd /src/tools/${crate} && cargo test --locked --offline --test coexist -- --exact phase"
  run_rust "$PROOF/${crate}-${phase}.log" -e "OO_A365_PHASE=${phase}"
}

if [ "$(cat "$PROOF/calendar-store-solo.log.exit" 2>/dev/null || true)" = "0" ]; then
  echo "calendar solo already passed; continue with resume-proof-2.sh" >&2
  exit 2
fi

run_phase calendar-store solo oo_a365_solo

if ! docker exec -u postgres "$PG_NAME" psql -U oo_fixture_admin -d postgres -Atc "SELECT 1 FROM pg_roles WHERE rolname = 'oo_enum_admin'" | grep -q 1; then
  enum_admin=$(cat "$SEC/enum-admin.pass")
  enum_reader=$(cat "$SEC/enum-reader.pass")
  cat > "$SEC/sql/02-enum-roles.sql" <<EOF
CREATE ROLE oo_enum_view NOLOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_enum_admin LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${enum_admin}';
GRANT oo_enum_view TO oo_enum_admin;
CREATE ROLE oo_r_enum LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${enum_reader}';
GRANT CONNECT, CREATE ON DATABASE oo_a365_solo TO oo_enum_admin;
GRANT CONNECT ON DATABASE oo_a365_solo TO oo_r_enum;
CREATE DATABASE oo_a365_cal_first OWNER oo_fixture_admin;
CREATE DATABASE oo_a365_enum_first OWNER oo_fixture_admin;
CREATE DATABASE oo_a365_hostile OWNER oo_fixture_admin;
REVOKE ALL ON DATABASE oo_a365_cal_first FROM PUBLIC;
REVOKE ALL ON DATABASE oo_a365_enum_first FROM PUBLIC;
REVOKE ALL ON DATABASE oo_a365_hostile FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE oo_a365_cal_first TO oo_calendar_admin, oo_enum_admin;
GRANT CONNECT, CREATE ON DATABASE oo_a365_enum_first TO oo_calendar_admin, oo_enum_admin;
GRANT CONNECT, CREATE ON DATABASE oo_a365_hostile TO oo_calendar_admin, oo_enum_admin;
GRANT CONNECT ON DATABASE oo_a365_cal_first TO oo_r_calendar, oo_r_enum, oo_r_other;
GRANT CONNECT ON DATABASE oo_a365_enum_first TO oo_r_calendar, oo_r_enum, oo_r_other;
GRANT CONNECT ON DATABASE oo_a365_hostile TO oo_r_calendar, oo_r_enum, oo_r_other;
EOF
  unset enum_admin enum_reader
  chmod 600 "$SEC/sql/02-enum-roles.sql"
  set +e
  docker exec -u postgres "$PG_NAME" psql -v ON_ERROR_STOP=1 -U oo_fixture_admin -d postgres -f /sql/02-enum-roles.sql > "$PROOF/enum-roles.log" 2>&1
  code=$?
  set -e
  printf '%s\n' "$code" > "$PROOF/enum-roles.log.exit"
  redact < "$PROOF/enum-roles.log" > "$PROOF/enum-roles.log.redacted"
  mv "$PROOF/enum-roles.log.redacted" "$PROOF/enum-roles.log"
  if [ "$code" -ne 0 ]; then
    echo "enum role sql failed" >&2
    exit "$code"
  fi
fi

run_phase enum-store solo_reject oo_a365_solo
run_phase calendar-store solo_replay oo_a365_solo
run_phase calendar-store cal_first oo_a365_cal_first
run_phase enum-store cal_first oo_a365_cal_first
run_phase calendar-store cal_first_replay oo_a365_cal_first
run_phase enum-store cal_first_replay oo_a365_cal_first
run_phase enum-store enum_first oo_a365_enum_first
run_phase calendar-store enum_first oo_a365_enum_first
run_phase enum-store enum_first_replay oo_a365_enum_first
run_phase calendar-store enum_first_replay oo_a365_enum_first
run_phase calendar-store hostile oo_a365_hostile
run_phase enum-store hostile oo_a365_hostile
run_phase calendar-store drift oo_a365_cal_first
run_phase enum-store drift oo_a365_enum_first

CMD='cd /src/tools/calendar-store && cargo clippy --locked --offline --all-targets -- -D warnings'
run_rust "$PROOF/calendar-clippy.log"
CMD='cd /src/tools/enum-store && cargo clippy --locked --offline --all-targets -- -D warnings'
run_rust "$PROOF/enum-clippy.log"

docker exec -u postgres "$PG_NAME" postgres --version > "$EVIDENCE/pg/version.txt"
printf '%s\n' 0 > "$PROOF/proof.exit"
echo "proof_ok"
