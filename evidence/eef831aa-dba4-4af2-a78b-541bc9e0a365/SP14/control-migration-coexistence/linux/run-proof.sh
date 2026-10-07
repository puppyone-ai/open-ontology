#!/bin/bash
# Attempt 1 only. Fetch failed before tests because oo-core had no workspace root.
# See proof/fetch-attempt-1/WHY.txt. The continued proof is linux/continue-proof.sh.
# One private PostgreSQL 17.10 and one Rust 1.94 builder for the coexistence proof.
# Origin network and postgres flags:
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a289/SP08/calendar-store/pg/up.sh
#   sha256 6eb5d3bc6106d52d7b41b0a1ae186d2e81063274868864e919b823b112e4229a
# Origin rust launch limits:
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a289/SP08/calendar-store/linux/impl-1/launch.sh
# Change: a365 names, both crates, phased OO_A365_PHASE, no host port, no host network.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a365
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/control-migration-coexistence
PROOF=${EVIDENCE}/proof
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a365-eef831aa-net
CARGO_VOL=oo-a365-eef831aa-cargo
TARGET_VOL=oo-a365-eef831aa-target
DATA_VOL=oo-a365-eef831aa-pgdata
PG_NAME=oo-a365-pg
BNAME=oo-a365-proof
TMP=/tmp/oo-a365-eef831aa-dba4-4af2-a78b-541bc9e0a365
SEC=${TMP}/secrets
FIX=${TMP}/fixture

redact() { sed -e 's/[0-9a-f]\{48\}/redacted/g' -e "s/PASSWORD '[^']*'/PASSWORD 'redacted'/g"; }

mkdir -p "$PROOF" "$EVIDENCE/pg"
if [ -f "$PROOF/proof.exit" ]; then
  echo "proof evidence already exists" >&2
  exit 2
fi
for image in "$RUST_IMAGE" "$PG_IMAGE"; do
  if ! docker image inspect "$image" >/dev/null 2>&1; then
    echo "pinned image is not present locally; not pulling" >&2
    printf '%s\n' 1 > "$PROOF/proof.exit"
    exit 1
  fi
done
docker image inspect "$RUST_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PROOF/rust-image.txt"
docker image inspect "$PG_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PROOF/pg-image.txt"
grep -q 'arch=arm64' "$PROOF/rust-image.txt"
grep -q 'arch=arm64' "$PROOF/pg-image.txt"
own=$(docker ps -q --filter label=oo.owner=a365 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a365 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s own_pg=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$own_pg" "$host_cargo" > "$PROOF/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ge 1 ]; then
  echo "deferred: builder or postgres capacity" >&2
  printf '%s\n' 3 > "$PROOF/proof.exit"
  exit 3
fi
if docker container inspect "$BNAME" >/dev/null 2>&1 || docker container inspect "$PG_NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$PROOF/proof.exit"
  exit 2
fi

rm -rf "$TMP"
umask 077
mkdir -p "$SEC/init" "$SEC/sql" "$FIX" "$EVIDENCE/pg"
chmod 700 "$SEC" "$SEC/init" "$SEC/sql" "$FIX"
openssl rand -hex 24 > "$SEC/fixture-admin.pass"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl rand -hex 24 > "$SEC/other.pass"
openssl rand -hex 24 > "$SEC/enum-admin.pass"
openssl rand -hex 24 > "$SEC/enum-reader.pass"
for pass in fixture-admin admin reader other enum-admin enum-reader; do
  grep -Eq '^[0-9a-f]{48}$' "$SEC/${pass}.pass"
  chmod 600 "$SEC/${pass}.pass"
done
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a365-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a365-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a365-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
find "$SEC" -maxdepth 1 -type f -exec chmod 600 {} +
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$EVIDENCE/pg/server-san.txt"
admin_pass=$(cat "$SEC/admin.pass")
reader_pass=$(cat "$SEC/reader.pass")
other_pass=$(cat "$SEC/other.pass")
cat > "$SEC/init/01-roles.sql" <<EOF
CREATE ROLE oo_calendar_view NOLOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_calendar_admin LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${admin_pass}';
GRANT oo_calendar_view TO oo_calendar_admin;
CREATE ROLE oo_r_calendar LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${reader_pass}';
CREATE ROLE oo_r_other LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${other_pass}';
CREATE DATABASE oo_a365_solo OWNER oo_fixture_admin;
REVOKE ALL ON DATABASE oo_a365_solo FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE oo_a365_solo TO oo_calendar_admin;
GRANT CONNECT ON DATABASE oo_a365_solo TO oo_r_calendar;
GRANT CONNECT ON DATABASE oo_a365_solo TO oo_r_other;
EOF
unset admin_pass reader_pass other_pass
chmod 600 "$SEC/init/01-roles.sql"
cp "$SEC/ca.crt" "$FIX/ca.crt"
cp "$SEC/admin.pass" "$FIX/admin.pass"
cp "$SEC/reader.pass" "$FIX/reader.pass"
cp "$SEC/other.pass" "$FIX/other.pass"
cp "$SEC/enum-admin.pass" "$FIX/enum-admin.pass"
cp "$SEC/enum-reader.pass" "$FIX/enum-reader.pass"
cp "$SEC/fixture-admin.pass" "$FIX/fixture-admin.pass"
printf '%s\n' oo-a365-pg 5432 oo_a365_solo > "$FIX/endpoint.txt"
chmod 644 "$FIX/ca.crt" "$FIX/endpoint.txt"
chmod 600 "$FIX/"*.pass

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a365 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format 'id={{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$PROOF/network.txt"
for vol in "$CARGO_VOL" "$TARGET_VOL" "$DATA_VOL"; do
  docker volume inspect "$vol" >/dev/null 2>&1 || docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a365 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=volume \
    "$vol" >/dev/null
done

docker run -d --name "$PG_NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add DAC_OVERRIDE \
  --cap-add FOWNER \
  --cap-add SETUID \
  --cap-add SETGID \
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias oo-a365-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/fixture-admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=bind,source=${SEC}/init,target=/docker-entrypoint-initdb.d,readonly" \
  --mount "type=bind,source=${SEC}/sql,target=/sql,readonly" \
  --mount "type=volume,source=${DATA_VOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${EVIDENCE}/pg/pg-wrap.sh,target=/fixture/pg-wrap.sh,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a365 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --entrypoint /fixture/pg-wrap.sh \
  "$PG_IMAGE" >/dev/null

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
  --mount "type=bind,source=${REPO}/tools/calendar-store,target=/src/tools/calendar-store,readonly" \
  --mount "type=bind,source=${REPO}/tools/business-calendar,target=/src/tools/business-calendar,readonly" \
  --mount "type=bind,source=${REPO}/tools/enum-store,target=/src/tools/enum-store,readonly" \
  --mount "type=bind,source=${REPO}/tools/enum-history,target=/src/tools/enum-history,readonly" \
  --mount "type=bind,source=${REPO}/crates/oo-core,target=/src/crates/oo-core,readonly" \
  --mount "type=bind,source=${REPO}/examples,target=/src/examples,readonly" \
  --mount "type=bind,source=${FIX},target=/run/oo-a365/fixture" \
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

printf '%s\n' "docker run one rust builder 6g 1cpu and one postgres 1g 1cpu on ${NET}; no host network; no published port" > "$PROOF/invocation.txt"
git -C "$REPO" rev-parse HEAD > "$PROOF/fixed-commit.txt"

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

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$PG_NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  docker logs "$PG_NAME" 2>&1 | redact > "$EVIDENCE/pg/postgres-start.log" || true
  printf '%s\n' 1 > "$PROOF/proof.exit"
  echo "postgres did not become ready" >&2
  exit 1
fi
printf '%s\n' 0 > "$EVIDENCE/pg/up.exit"
docker inspect "$PG_NAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$EVIDENCE/pg/hostconfig.txt"
docker inspect "$PG_NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/pg/ports.json"
docker inspect "$BNAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$PROOF/rust-hostconfig.txt"

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
if [ "$code" -ne 0 ]; then
  redact < "$PROOF/enum-roles.log" > "$PROOF/enum-roles.log.redacted"
  mv "$PROOF/enum-roles.log.redacted" "$PROOF/enum-roles.log"
  echo "enum role sql failed" >&2
  exit "$code"
fi
redact < "$PROOF/enum-roles.log" > "$PROOF/enum-roles.log.redacted"
mv "$PROOF/enum-roles.log.redacted" "$PROOF/enum-roles.log"

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
