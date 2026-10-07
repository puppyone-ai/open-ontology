#!/bin/bash
# Private PostgreSQL 17.10 and one Linux arm64 Rust 1.94 builder.
# Names, network, and volumes are UUID-scoped. Does not pull, prune, or publish.
set -euo pipefail

UUID=eef831aa-dba4-4af2-a78b-541bc9e0a358
REPO=$(cd "$(dirname "$0")/../../../../.." && pwd)
EVIDENCE="$REPO/evidence/${UUID}/SP07/review-ledger/proof-fix"
PG_DIR="$REPO/deploy/trusted-demo/pg"
SEC=/tmp/oo-a358-review-secrets
PG_NAME=oo-a358-review-pg
RUST_NAME=oo-a358-review-rust
NET=oo-a358-review-net
PG_VOL=oo-a358-review-pgvol
CARGO_VOL=oo-a358-review-cargo
TARGET_VOL=oo-a358-review-target
STATE_VOL=oo-a358-review-state
PG_HOST=pg.a358.internal
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
HOST_UID=$(id -u)
HOST_GID=$(id -g)

owned_label() {
  local name=$1
  local owner
  owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$name" 2>/dev/null || true)
  [ "$owner" = "$UUID" ]
}

refuse_foreign() {
  local name=$1
  if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
    if ! owned_label "$name"; then
      echo "foreign container $name" >&2
      exit 1
    fi
  fi
}

builders_running() {
  docker ps -q --filter label=oo.kind=rust-builder | wc -l | tr -d ' '
}

prepare_secrets() {
  umask 077
  mkdir -p "$SEC"
  chmod 700 "$SEC"
  if [ ! -f "$SEC/ca.crt" ]; then
    openssl req -x509 -newkey rsa:2048 -nodes \
      -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 30 \
      -subj '/CN=oo-a358-review-ca' >/dev/null 2>&1
    openssl req -newkey rsa:2048 -nodes \
      -keyout "$SEC/server.key" -out "$SEC/server.csr" \
      -subj '/CN=pg.a358.internal' >/dev/null 2>&1
    printf '%s\n' 'subjectAltName=DNS:pg.a358.internal' 'basicConstraints=CA:FALSE' > "$SEC/san.cnf"
    openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" \
      -CAcreateserial -out "$SEC/server.crt" -days 30 -extfile "$SEC/san.cnf" >/dev/null 2>&1
    openssl rand -hex 24 | awk '{printf "%s\n", $0}' > "$SEC/super.pass"
    openssl rand -hex 24 | awk '{printf "%s\n", $0}' > "$SEC/ledger-admin.pass"
    openssl rand -hex 24 | awk '{printf "%s\n", $0}' > "$SEC/runtime.pass"
    chmod 600 "$SEC"/*.pass "$SEC/server.key" "$SEC/ca.key"
    chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
  fi
  shasum -a 256 "$SEC/ca.crt" | awk '{print $1}' > "$EVIDENCE/ca.sha256"
}

up_pg() {
  prepare_secrets
  docker image inspect "$PG_IMAGE" >/dev/null
  docker image inspect "$PG_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/pg-image.txt"
  grep -q 'arch=arm64' "$EVIDENCE/pg-image.txt"
  if ! docker network inspect "$NET" >/dev/null 2>&1; then
    docker network create \
      --label oo.owner="$UUID" --label oo.session=a358 --label oo.kind=network --label oo.task=review-ledger \
      "$NET" >/dev/null
  fi
  docker volume create --label oo.owner="$UUID" --label oo.task=review-ledger "$PG_VOL" >/dev/null
  docker volume create --label oo.owner="$UUID" --label oo.task=review-ledger "$CARGO_VOL" >/dev/null
  docker volume create --label oo.owner="$UUID" --label oo.task=review-ledger "$TARGET_VOL" >/dev/null
  docker volume create --label oo.owner="$UUID" --label oo.task=review-ledger "$STATE_VOL" >/dev/null
  refuse_foreign "$PG_NAME"
  if docker ps -a --format '{{.Names}}' | grep -qx "$PG_NAME"; then
    echo "postgres container already exists"
    return
  fi
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
    --network-alias "$PG_HOST" \
    -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
    -e POSTGRES_USER=oo_a358_admin \
    -e POSTGRES_DB=postgres \
    -e POSTGRES_INITDB_ARGS=--data-checksums \
    -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
    --mount "type=bind,source=${SEC},target=/secrets,readonly" \
    --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
    --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
    --mount "type=bind,source=${PG_DIR},target=/fixture,readonly" \
    --label oo.owner="$UUID" \
    --label oo.session=a358 \
    --label oo.kind=postgres \
    --label oo.task=review-ledger \
    --entrypoint /fixture/pg-wrap.sh \
    "$PG_IMAGE" >/dev/null
  local ready=0
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
    if docker exec -u postgres "$PG_NAME" pg_isready -U oo_a358_admin -d postgres >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 2
  done
  if [ "$ready" -ne 1 ]; then
    docker logs "$PG_NAME" > "$EVIDENCE/postgres-start.log" 2>&1 || true
    echo "postgres did not become ready" >&2
    exit 1
  fi
  docker inspect "$PG_NAME" --format 'status={{.State.Status}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$EVIDENCE/pg-inspect.txt"
  docker inspect "$PG_NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/pg-ports.json"
  docker inspect "$PG_NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$EVIDENCE/pg-caps.json"
  bootstrap
}

bootstrap() {
  local admin_pw runtime_pw sql
  admin_pw=$(cat "$SEC/ledger-admin.pass")
  runtime_pw=$(cat "$SEC/runtime.pass")
  sql="$SEC/bootstrap.sql"
  cat > "$sql" <<EOF
DO \$\$ BEGIN
  CREATE ROLE oo_review_ledger_admin LOGIN PASSWORD '${admin_pw}';
EXCEPTION WHEN duplicate_object THEN NULL;
END \$\$;
DO \$\$ BEGIN
  CREATE ROLE oo_review_ledger_runtime NOLOGIN;
EXCEPTION WHEN duplicate_object THEN NULL;
END \$\$;
DO \$\$ BEGIN
  CREATE ROLE oo_a358_runtime LOGIN PASSWORD '${runtime_pw}';
EXCEPTION WHEN duplicate_object THEN NULL;
END \$\$;
GRANT oo_review_ledger_runtime TO oo_a358_runtime WITH INHERIT TRUE;
EOF
  docker exec -i -u postgres "$PG_NAME" psql -U oo_a358_admin -d postgres -v ON_ERROR_STOP=1 < "$sql" > "$EVIDENCE/bootstrap.out"
  rm -f "$sql"
  local exists
  exists=$(docker exec -u postgres "$PG_NAME" psql -U oo_a358_admin -d postgres -tAc "SELECT 1 FROM pg_database WHERE datname = 'oo_a358_review'")
  if [ "$exists" != "1" ]; then
    docker exec -u postgres "$PG_NAME" psql -U oo_a358_admin -d postgres -v ON_ERROR_STOP=1 \
      -c 'CREATE DATABASE oo_a358_review OWNER oo_review_ledger_admin' > "$EVIDENCE/database.out"
  fi
}

run_builder() {
  local phase=$1
  local log=$2
  refuse_foreign "$RUST_NAME"
  if docker ps -a --format '{{.Names}}' | grep -qx "$RUST_NAME"; then
    docker rm -f "$RUST_NAME" >/dev/null
  fi
  local running
  running=$(builders_running)
  if [ "$running" -ge 10 ]; then
    echo "rust builder cap reached: $running" >&2
    exit 1
  fi
  docker image inspect "$RUST_IMAGE" >/dev/null
  docker image inspect "$RUST_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/rust-image.txt"
  set +e
  docker run --name "$RUST_NAME" \
    --platform linux/arm64 \
    --pull never \
    --init \
    --security-opt no-new-privileges:true \
    --cap-drop ALL \
    --memory 6g \
    --memory-swap 6g \
    --cpus 1 \
    --pids-limit 512 \
    --network "$NET" \
    -e CARGO_HOME=/cargo \
    -e CARGO_TARGET_DIR=/target \
    -e CARGO_BUILD_JOBS=1 \
    -e CARGO_PROFILE_DEV_DEBUG=0 \
    -e CARGO_PROFILE_TEST_DEBUG=0 \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    -e REVIEW_LEDGER_HOST="$PG_HOST" \
    -e REVIEW_LEDGER_PORT=5432 \
    -e REVIEW_LEDGER_CA=/secrets/ca.crt \
    -e REVIEW_LEDGER_ADMIN_PASSWORD_FILE=/secrets/ledger-admin.pass \
    -e REVIEW_LEDGER_RUNTIME_PASSWORD_FILE=/secrets/runtime.pass \
    -e REVIEW_LEDGER_STATE=/state \
    -e REVIEW_LEDGER_MARKER=/src/evidence/${UUID}/SP07/review-ledger/proof-fix/history.sha256 \
    -e REVIEW_LEDGER_PHASE="$phase" \
    -e HOST_UID="$HOST_UID" \
    -e HOST_GID="$HOST_GID" \
    --mount "type=bind,source=${REPO},target=/src" \
    --mount "type=bind,source=${SEC},target=/secrets,readonly" \
    --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
    --mount "type=volume,source=${TARGET_VOL},target=/target" \
    --mount "type=volume,source=${STATE_VOL},target=/state" \
    --label oo.owner="$UUID" \
    --label oo.session=a358 \
    --label oo.kind=rust-builder \
    --label oo.task=review-ledger \
    -w /src/tools/review-ledger \
    "$RUST_IMAGE" \
    bash -c "$3" > "$log" 2>&1
  local code=$?
  set -e
  docker inspect "$RUST_NAME" --format 'exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$EVIDENCE/${phase}-inspect.txt"
  docker inspect "$RUST_NAME" --format '{{json .HostConfig.CapDrop}}' > "$EVIDENCE/${phase}-caps.json"
  docker rm "$RUST_NAME" >/dev/null
  return "$code"
}

test_cmd='set -eu
rustc --version
rustc --version | grep -q "rustc 1.94.0 "
cd /src/tools/review-ledger
rm -rf /target/debug /target/incremental
test -f Cargo.lock
cargo test --locked --lib --test contract --test live -- --test-threads=1
chown -R "$HOST_UID:$HOST_GID" /src/tools/review-ledger "/src/evidence/'"${UUID}"'/SP07/review-ledger" || true'

clippy_cmd='set -eu
rustc --version
rustc --version | grep -q "rustc 1.94.0 "
rustup component add clippy --toolchain 1.94.0
cd /src/tools/review-ledger
cargo clippy --locked --all-targets -- -D warnings
chown -R "$HOST_UID:$HOST_GID" /src/tools/review-ledger "/src/evidence/'"${UUID}"'/SP07/review-ledger" || true'

restart_cmd='set -eu
rustc --version
rustc --version | grep -q "rustc 1.94.0 "
cd /src/tools/review-ledger
cargo test --locked --offline --test live -- --test-threads=1
chown -R "$HOST_UID:$HOST_GID" "/src/evidence/'"${UUID}"'/SP07/review-ledger" || true'

mkdir -p "$EVIDENCE"
case "${1:-}" in
  up) up_pg ;;
  test)
    up_pg
    run_builder write "$EVIDENCE/write.log" "$test_cmd"
    ;;
  clippy)
    run_builder clippy "$EVIDENCE/clippy.log" "$clippy_cmd"
    ;;
  restart)
    docker restart "$PG_NAME" >/dev/null
    ready=0
    for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
      if docker exec -u postgres "$PG_NAME" pg_isready -U oo_a358_admin -d postgres >/dev/null 2>&1; then
        ready=1
        break
      fi
      sleep 2
    done
    [ "$ready" -eq 1 ]
    run_builder restart "$EVIDENCE/restart.log" "$restart_cmd"
    ;;
  *)
    echo "usage: launch.sh up|test|clippy|restart" >&2
    exit 2
    ;;
esac
