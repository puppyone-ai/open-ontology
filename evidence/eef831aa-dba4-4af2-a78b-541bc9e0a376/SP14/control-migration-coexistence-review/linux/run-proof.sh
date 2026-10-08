#!/bin/bash
# Independent a376 proof. One private PostgreSQL 17.10 and one Rust 1.94 builder.
# Frozen coexist tests require host oo-a365-pg, fixture /run/oo-a365/fixture, and
# database names oo_a365_*. Those strings are the product test contract.
# The container, network, volumes, and tmp directory are a376-private.
# The network alias oo-a365-pg points only at this session's database.
# Origin network and postgres flags:
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a289/SP08/calendar-store/pg/up.sh
#   commit 8a38387615981c6676b544ce60eb74cf00d3a725
#   sha256 6eb5d3bc6106d52d7b41b0a1ae186d2e81063274868864e919b823b112e4229a
# Origin rust launch limits:
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a289/SP08/calendar-store/linux/impl-1/launch.sh
#   sha256 e24fa5d5017d8f701e6189ff49ad787b4990c6313ea0d80c6bd802e5025b5327
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a293/SP08/enum-store/linux/impl-1/launch.sh
#   sha256 388f2cc7e177f52b705044a5d9134d445c3078454d3e9e7d010d2bd3917e2aff
# Change: a376 names, archive overlay, phased OO_A365_PHASE, catalog oracle.
# No host port, no host network, no cargo update, no root manifest edit.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a376
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/control-migration-coexistence-review
PROOF=${EVIDENCE}/proof
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a376-eef831aa-net
CARGO_VOL=oo-a376-eef831aa-cargo
TARGET_VOL=oo-a376-eef831aa-target
DATA_VOL=oo-a376-eef831aa-pgdata
PG_NAME=oo-a376-pg
BNAME=oo-a376-proof
TMP=/tmp/oo-a376-${RUN_ID}
SEC=${TMP}/secrets
FIX=${TMP}/fixture
ARCH=${TMP}/archive
REPAIR=be4b596481d83435df2ede8abd9ebb2a8647c706

redact() { sed -e 's/[0-9a-f]\{48\}/redacted/g' -e "s/PASSWORD '[^']*'/PASSWORD 'redacted'/g"; }
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
git -C "$REPO" diff --exit-code "$REPAIR" HEAD -- tools/calendar-store tools/enum-store
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
own=$(docker ps -q --filter label=oo.owner=a376 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a376 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
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
mkdir -p "$SEC/init" "$SEC/sql" "$FIX" "$ARCH"
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
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a376-ca" >/dev/null 2>&1
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

git -C "$REPO" archive --format=tar HEAD \
  Cargo.toml Cargo.lock rust-toolchain.toml \
  crates/oo-core tools/business-calendar tools/enum-history \
  examples/schema/calendar.yaml \
  | tar -x -C "$ARCH"
mkdir -p "$ARCH/tools"
cp -R "$REPO/tools/calendar-store" "$ARCH/tools/calendar-store"
cp -R "$REPO/tools/enum-store" "$ARCH/tools/enum-store"
rm -rf "$ARCH/tools/calendar-store/target" "$ARCH/tools/enum-store/target"
test -f "$ARCH/Cargo.toml"
test -f "$ARCH/crates/oo-core/Cargo.toml"
test -f "$ARCH/tools/business-calendar/Cargo.toml"
test -f "$ARCH/tools/enum-history/Cargo.toml"
test -f "$ARCH/examples/schema/calendar.yaml"
test -f "$ARCH/tools/calendar-store/Cargo.lock"
test -f "$ARCH/tools/enum-store/Cargo.lock"
git -C "$REPO" rev-parse HEAD > "$PROOF/archive-commit.txt"
printf '%s\n' "$REPAIR" > "$PROOF/repair-commit.txt"
git -C "$REPO" rev-parse 'HEAD:crates/oo-core' > "$PROOF/oo-core-tree.txt"
git -C "$REPO" rev-parse 'HEAD:tools/business-calendar' > "$PROOF/business-calendar-tree.txt"
git -C "$REPO" rev-parse 'HEAD:tools/enum-history' > "$PROOF/enum-history-tree.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a376 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format 'id={{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$PROOF/network.txt"
for vol in "$CARGO_VOL" "$TARGET_VOL" "$DATA_VOL"; do
  docker volume inspect "$vol" >/dev/null 2>&1 || docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a376 \
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
  --label oo.owner=a376 \
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
  --mount "type=bind,source=${FIX},target=/run/oo-a365/fixture" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a376 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=control-migration-coexistence-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  sleep infinity >/dev/null

printf '%s\n' "docker run one rust builder 6g 1cpu and one postgres 1g 1cpu on ${NET}; alias oo-a365-pg; no host network; no published port" > "$PROOF/invocation.txt"
git -C "$REPO" rev-parse HEAD > "$PROOF/fixed-commit.txt"
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
    exit "$code"
  fi
}

run_rust_keep() {
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
  printf '%s\n' "$code"
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

psql_q() {
  local db=$1
  local sql=$2
  docker exec -u postgres "$PG_NAME" psql -v ON_ERROR_STOP=1 -q -A -t -F '|' --no-psqlrc \
    -U oo_fixture_admin -d "$db" -c "$sql"
}

CMD='cd /src/tools/calendar-store && cargo test --locked --offline --lib --test contract'
mark calendar-lib
run_rust "$PROOF/calendar-lib-contract.log"
CMD='cd /src/tools/enum-store && cargo test --locked --offline --lib --test contract'
mark enum-lib
run_rust "$PROOF/enum-lib-contract.log"

set_db() { printf '%s\n' oo-a365-pg 5432 "$1" > "$FIX/endpoint.txt"; }
run_phase() {
  local crate=$1
  local phase=$2
  local db=$3
  mark "${crate}-${phase}"
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
mark enum-roles
set +e
docker exec -u postgres "$PG_NAME" psql -v ON_ERROR_STOP=1 -q --no-psqlrc -U oo_fixture_admin -d postgres -f /sql/02-enum-roles.sql > "$PROOF/enum-roles.log" 2>&1
code=$?
set -e
printf '%s\n' "$code" > "$PROOF/enum-roles.log.exit"
redact < "$PROOF/enum-roles.log" > "$PROOF/enum-roles.log.redacted"
mv "$PROOF/enum-roles.log.redacted" "$PROOF/enum-roles.log"
if [ "$code" -ne 0 ]; then
  echo "enum role sql failed" >&2
  exit "$code"
fi

mark roles-before
: > "$PROOF/facts-before.txt"
psql_q postgres "SELECT 'role', rolname, rolcanlogin, rolsuper, rolbypassrls, rolcreatedb, rolcreaterole FROM pg_catalog.pg_roles WHERE rolname IN ('oo_calendar_admin','oo_calendar_view','oo_enum_admin','oo_enum_view','oo_r_calendar','oo_r_enum') ORDER BY rolname" >> "$PROOF/facts-before.txt"
psql_q postgres "SELECT 'runtime_membership', count(*) FROM pg_catalog.pg_auth_members m JOIN pg_catalog.pg_roles g ON g.oid = m.roleid JOIN pg_catalog.pg_roles mem ON mem.oid = m.member WHERE mem.rolname IN ('oo_r_calendar','oo_r_enum') AND g.rolname IN ('oo_calendar_view','oo_enum_view','oo_calendar_admin','oo_enum_admin')" >> "$PROOF/facts-before.txt"
for db in oo_a365_cal_first oo_a365_enum_first oo_a365_hostile; do
  psql_q "$db" "SELECT 'before', current_database(), count(*) FROM pg_catalog.pg_namespace WHERE nspname IN ('oo_calendar_control','oo_enum_control','oo_dict')" >> "$PROOF/facts-before.txt"
done

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

mark facts-green
: > "$PROOF/facts-green.txt"
for db in oo_a365_cal_first oo_a365_enum_first; do
  psql_q "$db" "SELECT 'owner', current_database(), pg_catalog.pg_get_userbyid(nspowner) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_dict'" >> "$PROOF/facts-green.txt"
  psql_q "$db" "SELECT 'usage', current_database(), r.role, pg_catalog.has_schema_privilege(r.role, 'oo_dict', 'USAGE'), pg_catalog.has_schema_privilege(r.role, 'oo_dict', 'CREATE') FROM (VALUES ('oo_r_calendar'), ('oo_r_enum')) AS r(role)" >> "$PROOF/facts-green.txt"
  psql_q "$db" "SELECT 'view_create', current_database(), v.role, pg_catalog.has_schema_privilege(v.role, 'oo_dict', 'CREATE') FROM (VALUES ('oo_calendar_view'), ('oo_enum_view')) AS v(role)" >> "$PROOF/facts-green.txt"
  psql_q "$db" "SELECT 'exec', current_database(), r.role, f.sig, pg_catalog.has_function_privilege(r.role, f.sig, 'EXECUTE') FROM (VALUES ('oo_r_calendar'), ('oo_r_enum'), ('oo_r_other'), ('oo_calendar_admin'), ('oo_enum_admin')) AS r(role) CROSS JOIN (VALUES ('oo_calendar_control.dict_create_open()'), ('oo_calendar_control.dict_create_close()'), ('oo_calendar_control.dict_grant_runtime(text)'), ('oo_enum_control.dict_create_open()'), ('oo_enum_control.dict_create_close()'), ('oo_enum_control.dict_grant_runtime(text)')) AS f(sig)" >> "$PROOF/facts-green.txt"
  psql_q "$db" "SELECT 'allow', current_database(), 'oo_calendar_control', role_name FROM oo_calendar_control.runtime_allow UNION ALL SELECT 'allow', current_database(), 'oo_enum_control', role_name FROM oo_enum_control.runtime_allow" >> "$PROOF/facts-green.txt"
  psql_q "$db" "SELECT 'allow_count', current_database(), 'oo_calendar_control', count(*) FROM oo_calendar_control.runtime_allow UNION ALL SELECT 'allow_count', current_database(), 'oo_enum_control', count(*) FROM oo_enum_control.runtime_allow" >> "$PROOF/facts-green.txt"
  psql_q "$db" "SELECT 'fn', current_database(), n.nspname, p.proname, pg_catalog.pg_get_function_identity_arguments(p.oid), pg_catalog.pg_get_userbyid(p.proowner), p.prosecdef, l.lanname, COALESCE(p.proconfig::text, ''), (SELECT count(*) FROM pg_catalog.aclexplode(COALESCE(p.proacl, pg_catalog.acldefault('f', p.proowner))) a WHERE a.grantee = 0), p.prosrc FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace JOIN pg_catalog.pg_language l ON l.oid = p.prolang WHERE n.nspname IN ('oo_calendar_control','oo_enum_control') AND p.proname IN ('dict_create_open','dict_create_close','dict_grant_runtime') ORDER BY 3, 4" >> "$PROOF/facts-green.txt"
done
psql_q oo_a365_cal_first "BEGIN; SET LOCAL ROLE oo_enum_admin; GRANT USAGE ON SCHEMA oo_dict TO oo_r_other; RESET ROLE; SELECT 'noop', current_database(), 'oo_enum_admin', 'oo_r_other', pg_catalog.has_schema_privilege('oo_r_other', 'oo_dict', 'USAGE'); ROLLBACK" >> "$PROOF/facts-green.txt"
psql_q oo_a365_enum_first "BEGIN; SET LOCAL ROLE oo_calendar_admin; GRANT USAGE ON SCHEMA oo_dict TO oo_r_other; RESET ROLE; SELECT 'noop', current_database(), 'oo_calendar_admin', 'oo_r_other', pg_catalog.has_schema_privilege('oo_r_other', 'oo_dict', 'USAGE'); ROLLBACK" >> "$PROOF/facts-green.txt"

deny_one() {
  local db=$1
  local role=$2
  local schema=$3
  local kind=$4
  local stmt=$5
  psql_q "$db" "BEGIN; DO \$body\$ BEGIN BEGIN EXECUTE 'SET LOCAL ROLE ${role}'; ${stmt} RAISE EXCEPTION 'succeeded'; EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE '%', 'deny|${db}|${role}|${kind}|42501'; WHEN OTHERS THEN RAISE NOTICE '%', 'deny|${db}|${role}|${kind}|' || SQLSTATE; END; END \$body\$; ROLLBACK" >/dev/null
}
mark deny
: > "$PROOF/facts-deny.txt"
capture_deny() {
  local db=$1
  local role=$2
  local schema=$3
  docker exec -u postgres "$PG_NAME" psql -v ON_ERROR_STOP=1 -q --no-psqlrc -U oo_fixture_admin -d "$db" <<SQL > "$PROOF/facts-deny-${db}.log" 2>&1
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
    WHEN OTHERS THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|open|' || SQLSTATE;
  END;
  BEGIN
    EXECUTE 'SET LOCAL ROLE ${role}';
    PERFORM ${schema}.dict_grant_runtime('oo_r_other');
    RAISE EXCEPTION 'succeeded';
  EXCEPTION
    WHEN insufficient_privilege THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|grant|42501';
    WHEN OTHERS THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|grant|' || SQLSTATE;
  END;
  BEGIN
    EXECUTE 'SET LOCAL ROLE ${role}';
    EXECUTE 'CREATE TABLE oo_dict.a376_no (n integer)';
    RAISE EXCEPTION 'succeeded';
  EXCEPTION
    WHEN insufficient_privilege THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|create|42501';
    WHEN OTHERS THEN
      RAISE NOTICE '%', 'deny|${db}|${role}|create|' || SQLSTATE;
  END;
END
\$body\$;
ROLLBACK;
SQL
  sed -n 's/^NOTICE:  //p' "$PROOF/facts-deny-${db}.log" >> "$PROOF/facts-deny.txt"
}
capture_deny oo_a365_cal_first oo_r_enum oo_calendar_control
capture_deny oo_a365_enum_first oo_r_calendar oo_enum_control

mark pathdrift
psql_q oo_a365_cal_first "ALTER FUNCTION oo_calendar_control.dict_grant_runtime(text) SET search_path = pg_catalog, public" >/dev/null
psql_q oo_a365_cal_first "SELECT 'pathdrift|config', COALESCE(p.proconfig::text, '') FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = 'oo_calendar_control' AND p.proname = 'dict_grant_runtime'" > "$PROOF/facts-pathdrift.txt"
set_db oo_a365_cal_first
CMD="cd /src/tools/calendar-store && cargo test --locked --offline --test coexist -- --exact phase"
path_code=$(run_rust_keep "$PROOF/calendar-store-pathdrift.log" -e "OO_A365_PHASE=cal_first_replay")
if [ "$path_code" = 0 ]; then
  printf '%s\n' 'pathdrift|result|accepted' >> "$PROOF/facts-pathdrift.txt"
else
  if grep -q 'InstallDrift' "$PROOF/calendar-store-pathdrift.log"; then
    printf '%s\n' 'pathdrift|result|refused' >> "$PROOF/facts-pathdrift.txt"
  else
    printf '%s\n' 'pathdrift|result|other' >> "$PROOF/facts-pathdrift.txt"
  fi
fi

run_phase calendar-store hostile oo_a365_hostile
run_phase enum-store hostile oo_a365_hostile
run_phase calendar-store drift oo_a365_cal_first
run_phase enum-store drift oo_a365_enum_first

mark facts-refuse
: > "$PROOF/facts-refuse.txt"
psql_q oo_a365_hostile "SELECT 'hostile_private', count(*) FROM pg_catalog.pg_namespace WHERE nspname IN ('oo_calendar_control','oo_enum_control')" >> "$PROOF/facts-refuse.txt"
psql_q oo_a365_cal_first "SELECT 'drift', current_database(), 'enums', CASE WHEN COALESCE(c.reloptions::text, '') LIKE '%security_invoker=true%' THEN 'security_invoker=true' ELSE COALESCE(c.reloptions::text, '') END FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_dict' AND c.relname = 'enums'" >> "$PROOF/facts-refuse.txt"
psql_q oo_a365_enum_first "SELECT 'drift', current_database(), 'calendar', CASE WHEN COALESCE(c.reloptions::text, '') LIKE '%security_invoker=true%' THEN 'security_invoker=true' ELSE COALESCE(c.reloptions::text, '') END FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar'" >> "$PROOF/facts-refuse.txt"
psql_q oo_a365_cal_first "SELECT 'drift_schemas', current_database(), count(*) FROM pg_catalog.pg_namespace WHERE nspname IN ('oo_calendar_control','oo_enum_control')" >> "$PROOF/facts-refuse.txt"
psql_q oo_a365_enum_first "SELECT 'drift_schemas', current_database(), count(*) FROM pg_catalog.pg_namespace WHERE nspname IN ('oo_calendar_control','oo_enum_control')" >> "$PROOF/facts-refuse.txt"

mark raw-psql
docker exec -u postgres "$PG_NAME" psql -v ON_ERROR_STOP=1 -q --no-psqlrc -U oo_fixture_admin -d postgres <<'SQL' > "$PROOF/raw-create.log" 2>&1
CREATE DATABASE oo_a376_raw_tx OWNER oo_fixture_admin;
CREATE DATABASE oo_a376_raw_auto OWNER oo_fixture_admin;
REVOKE ALL ON DATABASE oo_a376_raw_tx FROM PUBLIC;
REVOKE ALL ON DATABASE oo_a376_raw_auto FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE oo_a376_raw_tx TO oo_calendar_admin, oo_enum_admin;
GRANT CONNECT, CREATE ON DATABASE oo_a376_raw_auto TO oo_calendar_admin, oo_enum_admin;
SQL
printf '%s\n' 0 > "$PROOF/raw-create.log.exit"
{
  printf '%s\n' 'SET ROLE oo_calendar_admin;'
  cat "$REPO/tools/calendar-store/migrations/oo_calendar_control_v1.sql"
} | docker exec -u postgres -i "$PG_NAME" psql -v ON_ERROR_STOP=1 --single-transaction -q --no-psqlrc \
  -U oo_fixture_admin -d oo_a376_raw_tx > "$PROOF/raw-tx-calendar.log" 2>&1
printf '%s\n' 0 > "$PROOF/raw-tx-calendar.log.exit"
set +e
{
  printf '%s\n' 'SET ROLE oo_enum_admin;'
  cat "$REPO/tools/enum-store/migrations/oo_enum_control_v1.sql"
} | docker exec -u postgres -i "$PG_NAME" psql -v ON_ERROR_STOP=1 --single-transaction --variable=VERBOSITY=verbose -q --no-psqlrc \
  -U oo_fixture_admin -d oo_a376_raw_tx > "$PROOF/raw-tx-enum.log" 2>&1
raw_tx_code=$?
set -e
printf '%s\n' "$raw_tx_code" > "$PROOF/raw-tx-enum.log.exit"
set +e
{
  printf '%s\n' 'SET ROLE oo_calendar_admin;'
  cat "$REPO/tools/calendar-store/migrations/oo_calendar_control_v1.sql"
} | docker exec -u postgres -i "$PG_NAME" psql -v ON_ERROR_STOP=1 -q --no-psqlrc \
  -U oo_fixture_admin -d oo_a376_raw_auto > "$PROOF/raw-auto-calendar.log" 2>&1
raw_auto_cal=$?
{
  printf '%s\n' 'SET ROLE oo_enum_admin;'
  cat "$REPO/tools/enum-store/migrations/oo_enum_control_v1.sql"
} | docker exec -u postgres -i "$PG_NAME" psql -v ON_ERROR_STOP=1 --variable=VERBOSITY=verbose -q --no-psqlrc \
  -U oo_fixture_admin -d oo_a376_raw_auto > "$PROOF/raw-auto-enum.log" 2>&1
raw_auto_enum=$?
set -e
printf '%s\n' "$raw_auto_cal" > "$PROOF/raw-auto-calendar.log.exit"
printf '%s\n' "$raw_auto_enum" > "$PROOF/raw-auto-enum.log.exit"
: > "$PROOF/facts-raw.txt"
sqlstate=$(sed -n 's/.*SQLSTATE: //p' "$PROOF/raw-tx-enum.log" | head -1)
printf '%s\n' "raw_tx|second|${sqlstate}" >> "$PROOF/facts-raw.txt"
psql_q oo_a376_raw_tx "SELECT 'raw_tx|oo_calendar_control', count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_calendar_control'" >> "$PROOF/facts-raw.txt"
psql_q oo_a376_raw_tx "SELECT 'raw_tx|oo_enum_control', count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_enum_control'" >> "$PROOF/facts-raw.txt"
psql_q oo_a376_raw_tx "SELECT 'raw_tx|oo_dict', count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_dict'" >> "$PROOF/facts-raw.txt"
psql_q oo_a376_raw_auto "SELECT 'raw_auto|oo_enum_control', count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_enum_control'" >> "$PROOF/facts-raw.txt"
psql_q oo_a376_raw_auto "SELECT 'raw_auto|dual', count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_dict' AND c.relname IN ('calendar','enums') AND c.relkind = 'v'" >> "$PROOF/facts-raw.txt"
printf '%s\n' "raw_auto|second_exit|${raw_auto_enum}" >> "$PROOF/facts-raw.txt"

CMD='cd /src/tools/calendar-store && cargo clippy --locked --offline --all-targets -- -D warnings'
mark clippy-calendar
run_rust "$PROOF/calendar-clippy.log"
CMD='cd /src/tools/enum-store && cargo clippy --locked --offline --all-targets -- -D warnings'
mark clippy-enum
run_rust "$PROOF/enum-clippy.log"

mark oracle
cat "$PROOF/facts-before.txt" "$PROOF/facts-green.txt" "$PROOF/facts-deny.txt" "$PROOF/facts-pathdrift.txt" "$PROOF/facts-refuse.txt" "$PROOF/facts-raw.txt" > "$PROOF/facts.txt"
CMD='rustc --edition 2021 -C opt-level=2 -o /target/a376-oracle /evidence/linux/oracle/main.rs && /target/a376-oracle /evidence/proof/facts.txt /src/tools/calendar-store/migrations/oo_calendar_control_v1.sql /src/tools/enum-store/migrations/oo_enum_control_v1.sql /src/tools/calendar-store/CONTRACT.md /src/tools/enum-store/CONTRACT.md'
run_rust "$PROOF/oracle.log"
printf '%s\n' 0 > "$PROOF/proof.exit"
mark proof_ok
echo proof_ok
