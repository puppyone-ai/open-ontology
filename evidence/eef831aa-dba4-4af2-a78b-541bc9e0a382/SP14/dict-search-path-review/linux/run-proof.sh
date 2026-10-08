#!/bin/bash
# One private PostgreSQL 17.10 and one Rust 1.94 builder.
# Focused search_path review. Does not run the two-order coexist matrix.
# Images are local. This script does not pull, prune, or publish a port.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a382
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/dict-search-path-review
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
FIXED=9a7940ba5bae734b16928cd68913b0037c262c0f
BASE=be4b596481d83435df2ede8abd9ebb2a8647c706
BNAME=oo-a382-proof
PG_NAME=oo-a382-pg
NET=oo-a382-eef831aa-net
CARGO_VOL=oo-a382-eef831aa-cargo
TARGET_VOL=oo-a382-eef831aa-target
DATA_VOL=oo-a382-eef831aa-pgdata
TMP=/tmp/oo-a382-${RUN_ID}
PROOF=${EVIDENCE}/proof
SEC=${TMP}/sec
FIX=${TMP}/fixture
ARCH=${TMP}/src

redact() { sed -E -e "s/PASSWORD '[^']*'/PASSWORD 'redacted'/g" -e 's/(^|[^0-9a-fA-F])[0-9a-fA-F]{48}([^0-9a-fA-F]|$)/\1redacted\2/g'; }
mark() { printf '%s %s\n' "$(date -u +%H:%M:%S)" "$1" | tee -a "$PROOF/progress.txt"; }

mkdir -p "$PROOF" "$EVIDENCE/pg" "$EVIDENCE/origin"
if [ -f "$PROOF/proof.exit" ]; then
  echo "proof evidence already exists" >&2
  exit 2
fi
if [ "$(cat "$EVIDENCE/linux/pre-edit-smoke/inside.exit")" != 0 ]; then
  echo "pre-edit smoke did not pass" >&2
  exit 2
fi
git -C "$REPO" diff --exit-code "$FIXED" HEAD -- tools/calendar-store tools/enum-store
git -C "$REPO" diff --exit-code "$FIXED" -- tools/calendar-store/migrations tools/enum-store/migrations tools/calendar-store/Cargo.lock tools/enum-store/Cargo.lock
git -C "$REPO" diff --exit-code "$BASE" "$FIXED" -- tools/calendar-store/migrations tools/enum-store/migrations tools/calendar-store/Cargo.lock tools/enum-store/Cargo.lock
git -C "$REPO" diff --stat "$BASE" "$FIXED" -- tools/calendar-store tools/enum-store > "$PROOF/product-delta-tracked.txt"
{
  echo "fixed=${FIXED}"
  echo "base=${BASE}"
  git -C "$REPO" show "${FIXED}:tools/calendar-store/src/sql.rs" | awk '/INSTALL_LOCK|DICT_LOCK/'
  git -C "$REPO" show "${FIXED}:tools/enum-store/src/sql.rs" | awk '/INSTALL_LOCK|DICT_LOCK/'
  echo "base-calendar"
  git -C "$REPO" show "${BASE}:tools/calendar-store/src/sql.rs" | awk '/INSTALL_LOCK|DICT_LOCK/'
  echo "base-enum"
  git -C "$REPO" show "${BASE}:tools/enum-store/src/sql.rs" | awk '/INSTALL_LOCK|DICT_LOCK/'
} > "$PROOF/locks.txt"
grep -q '881_275_289' "$PROOF/locks.txt"
grep -q '881_275_293' "$PROOF/locks.txt"
grep -q '881_275_280' "$PROOF/locks.txt"
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
own=$(docker ps -q --filter label=oo.owner=a382 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
pg_own=$(docker ps -q --filter label=oo.owner=a382 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s host_cargo=%s pg_own=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" "$pg_own" > "$PROOF/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$pg_own" -ge 1 ]; then
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
mkdir -p "$SEC/init" "$FIX" "$ARCH"
chmod 700 "$SEC" "$SEC/init" "$FIX"
openssl rand -hex 24 > "$SEC/fixture-admin.pass"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/enum-admin.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl rand -hex 24 > "$SEC/enum-reader.pass"
for pass in fixture-admin admin enum-admin reader enum-reader; do
  grep -Eq '^[0-9a-f]{48}$' "$SEC/${pass}.pass"
  chmod 600 "$SEC/${pass}.pass"
done
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a382-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a382-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a382-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
find "$SEC" -maxdepth 1 -type f -exec chmod 600 {} +
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$EVIDENCE/pg/server-san.txt"
admin_pass=$(cat "$SEC/admin.pass")
enum_admin=$(cat "$SEC/enum-admin.pass")
reader_pass=$(cat "$SEC/reader.pass")
enum_reader=$(cat "$SEC/enum-reader.pass")
cat > "$SEC/init/01-roles.sql" <<EOF
CREATE ROLE oo_calendar_view NOLOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_calendar_admin LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${admin_pass}';
GRANT oo_calendar_view TO oo_calendar_admin;
CREATE ROLE oo_r_calendar LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${reader_pass}';
CREATE ROLE oo_enum_view NOLOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_enum_admin LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${enum_admin}';
GRANT oo_enum_view TO oo_enum_admin;
CREATE ROLE oo_r_enum LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${enum_reader}';
CREATE DATABASE oo_a382_path OWNER oo_fixture_admin;
REVOKE ALL ON DATABASE oo_a382_path FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE oo_a382_path TO oo_calendar_admin, oo_enum_admin;
GRANT CONNECT ON DATABASE oo_a382_path TO oo_r_calendar, oo_r_enum;
EOF
unset admin_pass enum_admin reader_pass enum_reader
chmod 600 "$SEC/init/01-roles.sql"
cp "$SEC/ca.crt" "$FIX/ca.crt"
cp "$SEC/admin.pass" "$FIX/admin.pass"
cp "$SEC/enum-admin.pass" "$FIX/enum-admin.pass"
cp "$SEC/fixture-admin.pass" "$FIX/fixture-admin.pass"
printf '%s\n' oo-a382-pg 5432 oo_a382_path > "$FIX/endpoint.txt"
chmod 644 "$FIX/ca.crt" "$FIX/endpoint.txt"
chmod 600 "$FIX/"*.pass

git -C "$REPO" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml \
  crates/oo-core tools/business-calendar tools/enum-history \
  examples/schema/calendar.yaml \
  tools/calendar-store tools/enum-store \
  | tar -x -C "$ARCH"
rm -rf "$ARCH/tools/calendar-store/target" "$ARCH/tools/enum-store/target"
test -f "$ARCH/tools/calendar-store/tests/search_path.rs"
test -f "$ARCH/tools/enum-store/tests/search_path.rs"
test -f "$ARCH/crates/oo-core/Cargo.toml"
git -C "$REPO" rev-parse HEAD > "$PROOF/archive-commit.txt"
printf '%s\n' "$FIXED" > "$PROOF/fixed-commit.txt"
git -C "$REPO" rev-parse "${FIXED}:crates/oo-core" > "$PROOF/oo-core-tree.txt"
git -C "$REPO" rev-parse "${FIXED}:tools/business-calendar" > "$PROOF/business-calendar-tree.txt"
git -C "$REPO" rev-parse "${FIXED}:tools/enum-history" > "$PROOF/enum-history-tree.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a382 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format 'id={{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$PROOF/network.txt"
for vol in "$CARGO_VOL" "$TARGET_VOL" "$DATA_VOL"; do
  docker volume inspect "$vol" >/dev/null 2>&1 || docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a382 \
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
  --network-alias oo-a382-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/fixture-admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=bind,source=${SEC}/init,target=/docker-entrypoint-initdb.d,readonly" \
  --mount "type=volume,source=${DATA_VOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${EVIDENCE}/pg/pg-wrap.sh,target=/fixture/pg-wrap.sh,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a382 \
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
  --mount "type=bind,source=${ARCH},target=/src,readonly" \
  --mount "type=bind,source=${FIX},target=/run/oo-a378/fixture,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a382 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=dict-search-path-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  sleep infinity >/dev/null

printf '%s\n' "docker run one rust builder 6g 1cpu and one postgres 1g 1cpu on ${NET}; alias oo-a382-pg; no host network; no published port" > "$PROOF/invocation.txt"
mark started

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
    printf '%s\n' "$code" > "$PROOF/proof.exit"
    exit "$code"
  fi
}

CMD='rustc --version && cargo --version && rustup component add clippy --toolchain 1.94.0 && cargo fetch --locked --manifest-path /src/tools/calendar-store/Cargo.toml && cargo fetch --locked --manifest-path /src/tools/enum-store/Cargo.toml && echo cargo-update-invoked=no'
mark fetch
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
docker exec -u postgres "$PG_NAME" postgres --version > "$EVIDENCE/pg/version.txt"
docker exec -u postgres "$PG_NAME" psql -v ON_ERROR_STOP=1 -q -A -t -F '|' --no-psqlrc -U oo_fixture_admin -d postgres -c "SELECT rolname, rolcanlogin, rolsuper, rolbypassrls, rolcreatedb, rolcreaterole FROM pg_catalog.pg_roles WHERE rolname IN ('oo_calendar_admin','oo_calendar_view','oo_enum_admin','oo_enum_view','oo_r_calendar','oo_r_enum') ORDER BY 1" > "$PROOF/roles.txt"

CMD='cd /src && sha256sum tools/calendar-store/migrations/oo_calendar_control_v1.sql tools/enum-store/migrations/oo_enum_control_v1.sql tools/calendar-store/Cargo.lock tools/enum-store/Cargo.lock tools/calendar-store/src/adopt.rs tools/calendar-store/src/sql.rs tools/enum-store/src/adopt.rs tools/enum-store/src/sql.rs tools/calendar-store/tests/search_path.rs tools/enum-store/tests/search_path.rs'
mark source-sha
run_rust "$PROOF/source-sha256.txt"

CMD='cd /src/tools/calendar-store && cargo test --locked --offline --lib pinned_search_path_is_exact_pg_catalog_only -- --exact pinned_search_path_is_exact_pg_catalog_only'
mark calendar-unit
run_rust "$PROOF/calendar-unit.log"
CMD='cd /src/tools/enum-store && cargo test --locked --offline --lib pinned_search_path_is_exact_pg_catalog_only -- --exact pinned_search_path_is_exact_pg_catalog_only'
mark enum-unit
run_rust "$PROOF/enum-unit.log"

run_phase() {
  local crate=$1
  local name=$2
  mark "${crate}-${name}"
  CMD="cd /src/tools/${crate} && cargo test --locked --offline --test search_path -- --exact phase --nocapture"
  run_rust "$PROOF/${crate}-${name}.log" -e "OO_A378_PHASE=${name}"
}

run_phase calendar-store baseline
run_phase calendar-store reject_widened
run_phase enum-store adopt_collision
run_phase calendar-store restore_replay
run_phase enum-store shared_baseline
run_phase enum-store reject_widened
run_phase calendar-store sibling_drift
run_phase enum-store restore_replay
run_phase calendar-store sibling_replay

CMD='cd /src/tools/calendar-store && cargo clippy --locked --offline --all-targets -- -D warnings'
mark calendar-clippy
run_rust "$PROOF/calendar-clippy.log"
CMD='cd /src/tools/enum-store && cargo clippy --locked --offline --all-targets -- -D warnings'
mark enum-clippy
run_rust "$PROOF/enum-clippy.log"

if grep -n 'warning:' "$PROOF/calendar-clippy.log" "$PROOF/enum-clippy.log"; then
  echo "clippy warning" >&2
  printf '%s\n' 1 > "$PROOF/proof.exit"
  exit 1
fi
printf '%s\n' 0 > "$PROOF/proof.exit"
mark proof_ok
echo proof_ok
