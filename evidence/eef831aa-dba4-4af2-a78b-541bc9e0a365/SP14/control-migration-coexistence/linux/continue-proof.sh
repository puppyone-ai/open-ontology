#!/bin/bash
# Continue the a365 proof on the postgres container attempt 1 already started.
# Replace only the failed rust builder. Do not create a second postgres.
# Archive overlay origin:
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a289/SP08/calendar-store/linux/impl-1/launch.sh
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a293/SP08/enum-store/linux/impl-1/launch.sh
# Change: both installer working trees are overlaid, child Cargo.lock files are kept,
# and --locked --offline is used after one fetch. No cargo update. No root manifest edit.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a365
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/control-migration-coexistence
PROOF=${EVIDENCE}/proof
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a365-eef831aa-net
CARGO_VOL=oo-a365-eef831aa-cargo
TARGET_VOL=oo-a365-eef831aa-target
PG_NAME=oo-a365-pg
BNAME=oo-a365-proof
TMP=/tmp/oo-a365-eef831aa-dba4-4af2-a78b-541bc9e0a365
SEC=${TMP}/secrets
FIX=${TMP}/fixture
ARCH=${TMP}/archive

redact() { sed -e 's/[0-9a-f]\{48\}/redacted/g' -e "s/PASSWORD '[^']*'/PASSWORD 'redacted'/g"; }

if [ ! -f "$FIX/ca.crt" ] || [ ! -f "$SEC/enum-admin.pass" ]; then
  echo "private fixture is missing" >&2
  exit 2
fi
owner=$(docker inspect "$PG_NAME" --format '{{index .Config.Labels "oo.owner"}} {{index .Config.Labels "oo.session"}} {{index .Config.Labels "oo.kind"}}')
if [ "$owner" != "a365 ${RUN_ID} postgres" ]; then
  echo "refusing to reuse an unexpected postgres container" >&2
  exit 2
fi
if ! docker exec -u postgres "$PG_NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
  echo "existing postgres is not ready" >&2
  exit 1
fi

if docker container inspect "$BNAME" >/dev/null 2>&1; then
  bowner=$(docker inspect "$BNAME" --format '{{index .Config.Labels "oo.owner"}} {{index .Config.Labels "oo.session"}} {{index .Config.Labels "oo.kind"}}')
  if [ "$bowner" != "a365 ${RUN_ID} rust-builder" ]; then
    echo "refusing to remove an unexpected builder" >&2
    exit 2
  fi
  docker rm -f "$BNAME" >/dev/null
  printf '%s\n' removed > "$PROOF/fetch-attempt-1/builder-removed.txt"
fi

own=$(docker ps -q --filter label=oo.owner=a365 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a365 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s own_pg=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$own_pg" "$host_cargo" > "$PROOF/continue-capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ne 1 ]; then
  echo "deferred: builder or postgres capacity" >&2
  printf '%s\n' 3 > "$PROOF/proof.exit"
  exit 3
fi

rm -rf "$ARCH"
mkdir -p "$ARCH"
git -C "$REPO" archive --format=tar HEAD \
  Cargo.toml Cargo.lock rust-toolchain.toml \
  crates/oo-core tools/business-calendar tools/enum-history \
  examples/schema/calendar.yaml \
  | tar -x -C "$ARCH"
if [ ! -f "$ARCH/Cargo.toml" ] || [ ! -f "$ARCH/crates/oo-core/Cargo.toml" ] || [ ! -f "$ARCH/tools/business-calendar/Cargo.toml" ] || [ ! -f "$ARCH/tools/enum-history/Cargo.toml" ] || [ ! -f "$ARCH/examples/schema/calendar.yaml" ]; then
  echo "archive is missing a workspace path" >&2
  exit 2
fi
mkdir -p "$ARCH/tools"
cp -R "$REPO/tools/calendar-store" "$ARCH/tools/calendar-store"
cp -R "$REPO/tools/enum-store" "$ARCH/tools/enum-store"
rm -rf "$ARCH/tools/calendar-store/target" "$ARCH/tools/enum-store/target"
if [ ! -f "$ARCH/tools/calendar-store/Cargo.lock" ] || [ ! -f "$ARCH/tools/enum-store/Cargo.lock" ]; then
  echo "child Cargo.lock missing from the overlay" >&2
  exit 2
fi
git -C "$REPO" rev-parse HEAD > "$PROOF/archive-commit.txt"
git -C "$REPO" rev-parse 'HEAD:crates/oo-core' > "$PROOF/oo-core-tree.txt"
git -C "$REPO" rev-parse 'HEAD:tools/business-calendar' > "$PROOF/business-calendar-tree.txt"
git -C "$REPO" rev-parse 'HEAD:tools/enum-history' > "$PROOF/enum-history-tree.txt"

docker run -d --name "$BNAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ARCH},target=/src,readonly" \
  --mount "type=bind,source=${FIX},target=/run/oo-a365/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a365 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=control-migration-coexistence \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  sleep infinity >/dev/null

printf '%s\n' "replaced oo-a365-proof only; reused oo-a365-pg; archive overlay; 6g 1cpu; no host network" > "$PROOF/continue-invocation.txt"
docker inspect "$PG_NAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$EVIDENCE/pg/hostconfig.txt"
docker inspect "$PG_NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/pg/ports.json"
docker inspect "$BNAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$PROOF/rust-hostconfig.txt"
printf '%s\n' 0 > "$EVIDENCE/pg/up.exit"

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

CMD='rustc --version && cargo --version && rustup component add clippy --toolchain 1.94.0 && cargo fetch --locked --manifest-path /src/tools/calendar-store/Cargo.toml && cargo fetch --locked --manifest-path /src/tools/enum-store/Cargo.toml && echo cargo-update-invoked=no'
run_rust "$PROOF/fetch.log"

CMD='cd /src/tools/calendar-store && cargo test --locked --offline --lib --test contract'
run_rust "$PROOF/calendar-lib-contract.log"
CMD='cd /src/tools/enum-store && cargo test --locked --offline --lib --test contract'
run_rust "$PROOF/enum-lib-contract.log"

set_db() { printf '%s\n' oo-a365-pg 5432 "$1" > "$FIX/endpoint.txt"; }

run_phase() {
  local crate=$1
  local phase=$2
  local db=$3
  set_db "$db"
  CMD="cd /src/tools/${crate} && cargo test --locked --offline --test coexist -- --exact phase"
  run_rust "$PROOF/${crate}-${phase}.log" -e "OO_A365_PHASE=${phase}"
}

run_phase calendar-store solo oo_a365_solo

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
