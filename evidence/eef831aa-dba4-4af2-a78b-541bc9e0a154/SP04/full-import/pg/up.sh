#!/bin/bash
# One private PostgreSQL 17.10. Source and target are databases on this server.
# Secrets stay under /tmp. The private key is not mounted into the Rust builder.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a154/SP04/full-import/pg
ROOT=/tmp/oo-a154-eef831aa
SEC=${ROOT}/secrets
TLS=${ROOT}/tls
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NAME=oo-a154-pg
NET=oo-a154-eef831aa-net
VOL=oo-a154-eef831aa-pgdata

mkdir -p "$EVIDENCE"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  exit 1
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "a154 network is absent" >&2
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a154 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
if [ "$own" -ge 1 ]; then
  echo "this task already has a running postgres" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "named postgres is already running" >&2
    exit 3
  fi
  echo "refusing to remove an existing postgres container in status ${prior}" >&2
  exit 2
fi

umask 077
mkdir -p "$SEC" "$TLS"
chmod 700 "$SEC" "$TLS"
openssl rand -hex 24 > "$SEC/admin_password"
openssl rand -hex 24 > "$SEC/reader_password"
openssl rand -hex 24 > "$SEC/runtime_password"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$TLS/ca.key" -out "$TLS/ca.crt" -days 30 -subj "/CN=oo-a154-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$TLS/server.key" -out "$TLS/server.csr" -subj "/CN=oo-a154-pg" >/dev/null 2>&1
cat > "$TLS/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a154-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$TLS/server.csr" -CA "$TLS/ca.crt" -CAkey "$TLS/ca.key" -CAcreateserial -out "$TLS/server.crt" -days 30 -extfile "$TLS/san.cnf" >/dev/null 2>&1
cp "$TLS/ca.crt" "$SEC/ca.crt"
chmod 600 "$SEC"/* "$TLS"/*
chmod 700 "$SEC" "$TLS"

docker volume inspect "$VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a154 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a154 \
  --label oo.kind=postgres \
  "$VOL" >/dev/null

docker run -d \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add DAC_OVERRIDE \
  --cap-add FOWNER \
  --cap-add SETGID \
  --cap-add SETUID \
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias oo-a154-pg \
  -e POSTGRES_PASSWORD_FILE=/run/secrets/admin_password \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/run/secrets,readonly" \
  --mount "type=bind,source=${TLS},target=/tls-in,readonly" \
  --mount "type=volume,source=${VOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${EVIDENCE}/pg-wrap.sh,target=/pg-wrap.sh,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a154 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a154 \
  --label oo.task=full-import-pg \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  --entrypoint /pg-wrap.sh \
  "$IMAGE" \
  > "$EVIDENCE/container.id"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  docker logs "$NAME" > "$EVIDENCE/postgres.log" 2>&1 || true
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
docker inspect -f 'status={{.State.Status}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} capdrop={{json .HostConfig.CapDrop}} ports={{json .HostConfig.PortBindings}}' "$NAME" > "$EVIDENCE/hostconfig.txt"
printf '%s\n' "$NAME" > "$EVIDENCE/container-name.txt"
echo "postgres_ready name=$NAME"
