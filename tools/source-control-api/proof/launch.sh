#!/bin/bash
# One private PostgreSQL and one Rust builder. Reuses an existing oo-a383-pg.
# Refuses a second oo-a383-rust. Does not pull, prune, or touch foreign containers.
set -eu
umask 077
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
SEC=/tmp/oo-a383-secrets
LOG=/tmp/oo-a383-launch.log
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
: > "$LOG"
exec 2>>"$LOG"

fail() {
  printf '%s\n' "A383_FAILED exit=${1:-1}" | tee -a "$LOG"
  exit "${1:-1}"
}
note() { printf '%s\n' "$1" >> "$LOG"; }

docker image inspect "$RUST_IMAGE" >/dev/null || fail 1
docker image inspect "$PG_IMAGE" >/dev/null || fail 1

if docker container inspect oo-a383-rust >/dev/null 2>&1; then
  note "oo-a383-rust already exists"
  fail 1
fi

rid=$(docker image inspect -f '{{.Id}}' "$RUST_IMAGE")
count_rust() {
  local n=0 cid img
  for cid in $(docker ps -q); do
    img=$(docker inspect -f '{{.Image}}' "$cid")
    if [ "$img" = "$rid" ]; then n=$((n + 1)); fi
  done
  printf '%s\n' "$n"
}
running=$(count_rust)
note "rust_running=$running"
if [ "$running" -ge 10 ]; then fail 1; fi

mkdir -p "$SEC"
if [ ! -s "$SEC/server.key" ] || [ ! -s "$SEC/ca.crt" ]; then
  openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$SEC/ca.key"
  openssl req -batch -new -x509 -key "$SEC/ca.key" -out "$SEC/ca.crt" -days 30 \
    -subj "/CN=oo-a383-ca" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign"
  openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$SEC/server.key"
  openssl req -batch -new -key "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a383-pg"
  cat > "$SEC/server.ext" << 'EOF'
basicConstraints=critical,CA:FALSE
subjectAltName=DNS:oo-a383-pg,IP:127.0.0.1
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial \
    -out "$SEC/server.crt" -days 30 -extfile "$SEC/server.ext"
  rm -f "$SEC/ca.key" "$SEC/server.csr" "$SEC/server.ext" "$SEC/ca.srl"
fi
grep -q "BEGIN PRIVATE KEY" "$SEC/server.key" || fail 1
openssl x509 -in "$SEC/ca.crt" -noout -text | grep -q "CA:TRUE" || fail 1
openssl x509 -in "$SEC/server.crt" -noout -text | grep -q "CA:FALSE" || fail 1
openssl x509 -in "$SEC/server.crt" -noout -ext subjectAltName | grep -q "DNS:oo-a383-pg" || fail 1
openssl verify -CAfile "$SEC/ca.crt" "$SEC/server.crt" >/dev/null || fail 1
for name in admin.pass api.pass identity.pass reader.pass writer.pass runtime.pass client.secret; do
  if [ ! -s "$SEC/$name" ]; then
    printf '%s' "$(openssl rand -hex 16)" > "$SEC/$name"
  fi
done

if ! docker network inspect oo-a383-net >/dev/null 2>&1; then
  docker network create --driver bridge oo-a383-net >/dev/null
fi
docker volume create oo-a383-pgdata >/dev/null
docker volume create oo-a383-cargo >/dev/null
docker volume create oo-a383-target >/dev/null

if docker container inspect oo-a383-pg >/dev/null 2>&1; then
  pg_state=$(docker inspect -f '{{.State.Status}}' oo-a383-pg | tr -d '[:space:]')
else
  pg_state=missing
fi
note "pg_state=$pg_state"
case "$pg_state" in
  running) ;;
  exited|created) docker start oo-a383-pg >/dev/null ;;
  missing)
    docker run -d --name oo-a383-pg \
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
      --network oo-a383-net \
      --network-alias oo-a383-pg \
      -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
      -e POSTGRES_USER=oo_fixture_admin \
      -e POSTGRES_DB=postgres \
      -e POSTGRES_INITDB_ARGS=--data-checksums \
      -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
      --mount "type=bind,source=${SEC},target=/secrets,readonly" \
      --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
      --mount "type=volume,source=oo-a383-pgdata,target=/var/lib/postgresql/data" \
      --mount "type=bind,source=${ROOT}/tools/source-control-api/proof,target=/fixture,readonly" \
      --label oo.owner=eef831aa-dba4-4af2-a78b-541bc9e0a383 \
      --label oo.kind=postgres \
      --entrypoint /fixture/pg-wrap.sh \
      "$PG_IMAGE" >/dev/null
    ;;
  *) fail 1 ;;
esac

ready=0
for _ in $(seq 1 60); do
  if docker exec oo-a383-pg pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  note "pg not ready"
  fail 1
fi

running=$(count_rust)
note "rust_running_before=$running"
if [ "$running" -ge 10 ]; then fail 1; fi

set +e
docker run --name oo-a383-rust \
  --platform linux/arm64 \
  --pull never \
  --network oo-a383-net \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --mount "type=bind,src=${ROOT},dst=/src" \
  --mount "type=volume,src=oo-a383-cargo,dst=/cargo-home" \
  --mount "type=volume,src=oo-a383-target,dst=/cargo/target" \
  --mount "type=bind,src=${SEC},dst=/secrets" \
  --tmpfs /tmp:rw,nosuid,nodev,size=2g \
  -w /src \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/cargo/target \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_PROFILE_DEV_DEBUG=0 \
  -e CARGO_PROFILE_DEV_OPT_LEVEL=1 \
  -e "HOST_UID=$(id -u)" \
  -e "HOST_GID=$(id -g)" \
  --label oo.owner=eef831aa-dba4-4af2-a78b-541bc9e0a383 \
  --label oo.kind=source-control-api \
  "$RUST_IMAGE" \
  bash /src/tools/source-control-api/proof/inside.sh >> "$LOG" 2>&1
code=$?
set -e
note "container_exit=$code"
if grep -q '^A383_DONE exit=0$' "$LOG"; then
  printf '%s\n' "A383_DONE exit=0" | tee -a "$LOG"
  exit 0
fi
printf '%s\n' "A383_FAILED exit=$code" | tee -a "$LOG"
exit "$code"
