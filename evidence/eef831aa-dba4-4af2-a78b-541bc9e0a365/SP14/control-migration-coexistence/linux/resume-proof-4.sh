#!/bin/bash
# Rebuild oo_a365_cal_first and continue from calendar cal_first.
# The previous calendar install exited 0 before the manifest SELECT grant existed.
# The enum adopter then returned collision because it could not read the digest.
# Do not docker run. Do not rerun solo, solo_reject, or solo_replay.
echo "resume-proof-4 already rebuilt oo_a365_cal_first; continue with resume-proof-5.sh" >&2
exit 2
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a365
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/control-migration-coexistence
PROOF=${EVIDENCE}/proof
PG_NAME=oo-a365-pg
BNAME=oo-a365-proof
TMP=/tmp/oo-a365-eef831aa-dba4-4af2-a78b-541bc9e0a365
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
for need in fetch.log.exit calendar-lib-contract.log.exit enum-lib-contract.log.exit calendar-store-solo.log.exit enum-roles.log.exit enum-store-solo_reject.log.exit calendar-store-solo_replay.log.exit; do
  if [ "$(cat "$PROOF/$need" 2>/dev/null || true)" != "0" ]; then
    echo "refusing to skip a phase that did not exit 0: $need" >&2
    exit 2
  fi
done

for rel in src/sql.rs src/adopt.rs src/store.rs; do
  cp "$REPO/tools/calendar-store/$rel" "$ARCH/tools/calendar-store/$rel"
  cp "$REPO/tools/enum-store/$rel" "$ARCH/tools/enum-store/$rel"
done
cp "$REPO/tools/calendar-store/CONTRACT.md" "$ARCH/tools/calendar-store/CONTRACT.md"
cp "$REPO/tools/enum-store/CONTRACT.md" "$ARCH/tools/enum-store/CONTRACT.md"

docker exec -u postgres -i "$PG_NAME" psql -v ON_ERROR_STOP=1 -U oo_fixture_admin -d postgres <<'SQL' > "$PROOF/reset-cal-first.log" 2>&1
SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = 'oo_a365_cal_first' AND pid <> pg_backend_pid();
DROP DATABASE IF EXISTS oo_a365_cal_first;
CREATE DATABASE oo_a365_cal_first OWNER oo_fixture_admin;
REVOKE ALL ON DATABASE oo_a365_cal_first FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE oo_a365_cal_first TO oo_calendar_admin, oo_enum_admin;
GRANT CONNECT ON DATABASE oo_a365_cal_first TO oo_r_calendar, oo_r_enum, oo_r_other;
SQL
printf '%s\n' 0 > "$PROOF/reset-cal-first.log.exit"

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
