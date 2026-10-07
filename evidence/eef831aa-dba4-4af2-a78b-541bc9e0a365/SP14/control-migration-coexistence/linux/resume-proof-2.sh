#!/bin/bash
# Continue the existing a365 builder and postgres after enum solo_reject exit 101.
# Do not docker run. Do not rerun smoke, fetch, lib, contract, calendar solo, or enum roles.
# Copy the repaired installer sources into the archive the builder already mounts.
# This script already ran: enum solo_reject exited 0 and calendar solo_replay exited 101.
# A later resume that finds enum solo_reject exit 0 must use resume-proof-3.sh.
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
for need in fetch.log.exit calendar-lib-contract.log.exit enum-lib-contract.log.exit calendar-store-solo.log.exit enum-roles.log.exit; do
  if [ "$(cat "$PROOF/$need" 2>/dev/null || true)" != "0" ]; then
    echo "refusing to skip a phase that did not exit 0: $need" >&2
    exit 2
  fi
done
if ! docker exec -u postgres "$PG_NAME" psql -U oo_fixture_admin -d postgres -Atc "SELECT 1 FROM pg_roles WHERE rolname = 'oo_enum_admin'" | grep -q 1; then
  echo "enum roles are absent; refusing to recreate them here" >&2
  exit 2
fi

for rel in src/sql.rs src/adopt.rs src/store.rs; do
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

if [ "$(cat "$PROOF/enum-store-solo_reject.log.exit" 2>/dev/null || true)" = "0" ]; then
  echo "enum solo_reject already passed; continue with resume-proof-3.sh" >&2
  exit 2
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
