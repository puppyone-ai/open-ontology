#!/bin/bash
# Two PostgreSQL 17.10 instances for the acceptance fixture.
# Source receives the public rows. Target stays empty of those rows.
# Origin server wrapper: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/pg/pg-wrap.sh
# sha256 d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb
# Origin up.sh: same review, pg/up.sh sha256 ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76
# Changes: owner a407, two instances, 1 GiB and 1 CPU each, distinct SAN names, no host port.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a407
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP11/minimal-loop-acceptance/fixture/pg
SEC=/tmp/oo-a407-${RUN_ID}/secrets
HERE=$(cd "$(dirname "$0")" && pwd)
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a407-eef831aa-net
WRAP=${HERE}/pg-wrap.sh

mkdir -p "$PG"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"
own_pg=$(docker ps -q --filter label=oo.owner=a407 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s cap=2\n' "$own_pg" > "$PG/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$PG/up.exit"
  exit 3
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a407 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" >/dev/null || docker network inspect "$NET" >/dev/null
fi

start_one() {
  role=$1
  name=$2
  database=$3
  volume=$4
  alias=$5
  sec=${SEC}/${role}
  if docker ps -a --filter "name=^/${name}$" --format '{{.Status}}' | grep -q .; then
    echo "pg container ${name} already exists" >&2
    printf '%s\n' 2 > "$PG/up.exit"
    exit 2
  fi
  umask 077
  mkdir -p "$sec"
  chmod 700 "$sec"
  openssl rand -hex 24 > "$sec/admin.pass"
  openssl rand -hex 24 > "$sec/reader.pass"
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$sec/ca.key" -out "$sec/ca.crt" -days 7 -subj "/CN=oo-a407-${role}-ca" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$sec/server.key" -out "$sec/server.csr" -subj "/CN=${alias}" >/dev/null 2>&1
  cat > "$sec/san.cnf" <<EOF
basicConstraints=CA:FALSE
subjectAltName=DNS:${alias}
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "$sec/server.csr" -CA "$sec/ca.crt" -CAkey "$sec/ca.key" -CAcreateserial -out "$sec/server.crt" -days 7 -extfile "$sec/san.cnf" >/dev/null 2>&1
  chmod 600 "$sec"/*
  chmod 644 "$sec/ca.crt" "$sec/server.crt"
  chmod 600 "$sec/server.key" "$sec/ca.key" "$sec"/*.pass
  openssl x509 -in "$sec/server.crt" -noout -subject -ext subjectAltName > "$PG/${role}-san.txt"
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a407 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=postgres \
    "$volume" > "$PG/${role}-volume.txt"
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
    --network-alias "$alias" \
    -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
    -e POSTGRES_USER=oo_a407_admin \
    -e POSTGRES_DB="$database" \
    -e POSTGRES_INITDB_ARGS=--data-checksums \
    -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
    --mount "type=bind,source=${sec},target=/secrets,readonly" \
    --mount "type=bind,source=${sec},target=/tls-in,readonly" \
    --mount "type=volume,source=${volume},target=/var/lib/postgresql/data" \
    --mount "type=bind,source=${WRAP},target=/fixture/pg-wrap.sh,readonly" \
    --label oo.project=open-ontology \
    --label oo.owner=a407 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=postgres \
    --label oo.task=minimal-loop-acceptance-fixture \
    --entrypoint /fixture/pg-wrap.sh \
    "$IMAGE" > "$PG/${role}-container.id"
  ready=0
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
    if docker exec -u postgres "$name" pg_isready -U oo_a407_admin -d "$database" >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 2
  done
  if [ "$ready" -ne 1 ]; then
    echo "postgres ${name} did not become ready" >&2
    docker logs "$name" > "$PG/${role}-start.log" 2>&1 || true
    printf '%s\n' 1 > "$PG/up.exit"
    exit 1
  fi
  docker inspect "$name" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}}' > "$PG/${role}-inspect.txt"
  docker inspect "$name" --format '{{json .HostConfig.PortBindings}}' > "$PG/${role}-ports.json"
  docker inspect "$name" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PG/${role}-caps.json"
}

start_one src oo-a407-src-pg oo_a407_src oo-a407-eef831aa-src-pgdata oo-a407-src-pg
start_one dst oo-a407-dst-pg oo_a407_dst oo-a407-eef831aa-dst-pgdata oo-a407-dst-pg
printf '%s\n' "$SEC" > "$PG/secrets-path.txt"
printf '%s\n' 0 > "$PG/up.exit"
echo "postgres_ready source=oo-a407-src-pg target=oo-a407-dst-pg"
