#!/bin/bash
# Two private PostgreSQL instances for a409's own copy proof.
# Does not stop other owners. Does not use a410 secrets or databases.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a409
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP03/local-copy-acceptance/linux/copy/pg
WRAP=${REPO}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/pg-wrap.sh
SEC=/tmp/oo-a409-${RUN_ID}/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a409-eef831aa-net
WRAP_HASH=25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f

mkdir -p "$PG"
if [ -f "$PG/up.exit" ] && [ "$(cat "$PG/up.exit")" = "0" ] && docker ps --format '{{.Names}}' | grep -qx oo-a409-src && docker ps --format '{{.Names}}' | grep -qx oo-a409-tgt; then
  echo "postgres pair already ready"
  exit 0
fi
got=$(shasum -a 256 "$WRAP" | awk '{print $1}')
if [ "$got" != "$WRAP_HASH" ]; then
  echo "pg-wrap.sh hash drifted" >&2
  printf '%s\n' 2 > "$PG/up.exit"
  exit 2
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a409 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null
own_pg=$(docker ps -q --filter label=oo.owner=a409 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s cap_own=2 memory=1g cpus=1\n' "$own_pg" > "$PG/capacity.txt"
if [ "$own_pg" -ge 2 ]; then
  echo "deferred: this task already has two postgres containers" >&2
  printf '%s\n' 3 > "$PG/up.exit"
  exit 3
fi
if [ "$own_pg" -ne 0 ]; then
  echo "refusing a partial postgres pair" >&2
  printf '%s\n' 2 > "$PG/up.exit"
  exit 2
fi

umask 077
mkdir -p "$SEC/src" "$SEC/tgt"
chmod 700 "$SEC" "$SEC/src" "$SEC/tgt"
openssl rand -hex 24 > "$SEC/src/admin.pass"
openssl rand -hex 24 > "$SEC/tgt/admin.pass"
openssl rand -hex 24 > "$SEC/src.pass"
openssl rand -hex 24 > "$SEC/runtime.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a409-ca" >/dev/null 2>&1
issue() {
  local dir="$1" name="$2"
  openssl req -newkey rsa:2048 -nodes -keyout "$dir/server.key" -out "$dir/server.csr" -subj "/CN=${name}" >/dev/null 2>&1
  cat > "$dir/san.cnf" <<EOF
basicConstraints=CA:FALSE
subjectAltName=DNS:${name}
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "$dir/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$dir/server.crt" -days 7 -extfile "$dir/san.cnf" >/dev/null 2>&1
  cp "$SEC/ca.crt" "$dir/ca.crt"
  chmod 600 "$dir/server.key" "$dir/admin.pass"
  chmod 644 "$dir/server.crt" "$dir/ca.crt"
  openssl x509 -in "$dir/server.crt" -noout -subject -ext subjectAltName > "$PG/${name}-san.txt"
}
issue "$SEC/src" oo-a409-src
issue "$SEC/tgt" oo-a409-tgt
chmod 600 "$SEC/src.pass" "$SEC/runtime.pass" "$SEC/reader.pass" "$SEC/ca.key"
chmod 644 "$SEC/ca.crt"
shasum -a 256 "$SEC/ca.crt" | awk '{print $1}' > "$PG/ca.sha256"

start_one() {
  local name="$1" vol="$2" sec="$3"
  if docker ps -a --filter "name=^/${name}$" --format '{{.Names}}' | grep -qx "$name"; then
    echo "pg container name already exists: $name" >&2
    printf '%s\n' 2 > "$PG/up.exit"
    exit 2
  fi
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a409 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=postgres \
    "$vol" > "$PG/${name}-volume.txt"
  docker run -d --name "$name" \
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
    --network-alias "$name" \
    -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
    -e POSTGRES_USER=oo_fixture_admin \
    -e POSTGRES_DB=postgres \
    -e POSTGRES_INITDB_ARGS=--data-checksums \
    -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
    --mount "type=bind,source=${sec},target=/secrets,readonly" \
    --mount "type=bind,source=${sec},target=/tls-in,readonly" \
    --mount "type=volume,source=${vol},target=/var/lib/postgresql/data" \
    --mount "type=bind,source=${WRAP},target=/fixture/pg-wrap.sh,readonly" \
    --label oo.project=open-ontology \
    --label oo.owner=a409 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=postgres \
    --label oo.task=local-copy-acceptance \
    --entrypoint /fixture/pg-wrap.sh \
    "$IMAGE" > "$PG/${name}.id"
}
start_one oo-a409-src oo-a409-eef831aa-srcdata "$SEC/src"
start_one oo-a409-tgt oo-a409-eef831aa-tgtdata "$SEC/tgt"

ready_one() {
  local name="$1"
  local ready=0
  local _
  for _ in $(seq 1 40); do
    if docker exec -u postgres "$name" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 2
  done
  if [ "$ready" -ne 1 ]; then
    echo "postgres did not become ready: $name" >&2
    docker logs "$name" > "$PG/${name}-start.log" 2>&1 || true
    printf '%s\n' 1 > "$PG/up.exit"
    exit 1
  fi
  docker inspect "$name" --format 'id={{.Id}} status={{.State.Status}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} cpus={{.HostConfig.NanoCpus}}' > "$PG/${name}-inspect.txt"
  docker inspect "$name" --format '{{json .HostConfig.PortBindings}}' > "$PG/${name}-ports.json"
  docker inspect "$name" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PG/${name}-caps.json"
}
ready_one oo-a409-src
ready_one oo-a409-tgt
# Official image with POSTGRES_USER=oo_fixture_admin does not create the postgres role.
# Local peer still maps the OS user postgres to that role name, so the monitor needs the role.
bootstrap_postgres_role() {
  local name="$1"
  docker exec -u postgres "$name" bash -c 'export PGPASSWORD=$(tr -d "\n" < /secrets/admin.pass); psql "host=127.0.0.1 user=oo_fixture_admin dbname=postgres sslmode=disable" -v ON_ERROR_STOP=1 -c "CREATE ROLE postgres SUPERUSER LOGIN;"'
}
bootstrap_postgres_role oo-a409-src
bootstrap_postgres_role oo-a409-tgt
printf '%s\n' 0 > "$PG/up.exit"
echo "postgres_ready names=oo-a409-src,oo-a409-tgt"
