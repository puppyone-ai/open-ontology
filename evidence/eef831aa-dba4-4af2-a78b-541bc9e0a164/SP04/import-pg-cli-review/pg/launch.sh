#!/bin/bash
# One private PostgreSQL 17.10 for the a164 review. The server key stays in /tmp.
set -u
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a164/SP04/import-pg-cli-review/pg
ROOT=/tmp/oo-a164-eef831aa-dba4-4af2-a78b-541bc9e0a164
SEC=${ROOT}/secrets
TLS=${ROOT}/tls
CA=${ROOT}/ca
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NAME=oo-a164-pg
NET=oo-a164-eef831aa-net
VOL=oo-a164-eef831aa-pgdata

chmod 755 "$EVIDENCE/pg-wrap.sh" "$EVIDENCE/launch.sh"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  exit 1
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "a164 network is absent" >&2
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a164 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
if [ "$own" -ge 1 ]; then
  echo "this task already has a running postgres" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  echo "refusing to replace an existing postgres container in status ${prior}" >&2
  exit 2
fi

umask 077
mkdir -p "$SEC" "$TLS" "$CA"
chmod 700 "$SEC" "$TLS" "$CA"
openssl rand -hex 24 > "$SEC/admin_password"
openssl rand -hex 24 > "$SEC/reader_password"
openssl rand -hex 24 > "$SEC/writer_password"
openssl rand -hex 24 > "$SEC/runtime_password"
openssl rand -hex 24 > "$SEC/agent_password"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$TLS/ca.key" -out "$TLS/ca.crt" -days 30 -subj "/CN=oo-a164-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$TLS/server.key" -out "$TLS/server.csr" -subj "/CN=oo-a164-pg" >/dev/null 2>&1
cat > "$TLS/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a164-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$TLS/server.csr" -CA "$TLS/ca.crt" -CAkey "$TLS/ca.key" -CAcreateserial -out "$TLS/server.crt" -days 30 -extfile "$TLS/san.cnf" >/dev/null 2>&1
cp "$TLS/ca.crt" "$CA/ca.crt"
chmod 600 "$SEC"/* "$TLS"/*
chmod 644 "$CA/ca.crt"
chmod 700 "$SEC" "$TLS" "$CA"
openssl x509 -in "$CA/ca.crt" -noout -fingerprint -sha256 > "$EVIDENCE/ca-fingerprint.txt"

docker volume inspect "$VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a164 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a164 \
  --label oo.kind=postgres \
  "$VOL" >/dev/null

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
printf '%s\n' "docker run --name $NAME --platform linux/arm64 --pull never --network $NET --cap-drop ALL image=pinned-postgres-17.10" > "$EVIDENCE/invocation.txt"

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
  --network-alias oo-a164-pg \
  -e POSTGRES_PASSWORD_FILE=/run/secrets/admin_password \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC}/admin_password,target=/run/secrets/admin_password,readonly" \
  --mount "type=bind,source=${TLS},target=/tls-in,readonly" \
  --mount "type=volume,source=${VOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${EVIDENCE}/pg-wrap.sh,target=/pg-wrap.sh,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a164 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a164 \
  --label oo.task=import-pg-cli-review \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  --entrypoint /pg-wrap.sh \
  "$IMAGE" >/dev/null

printf '%s\n' "$NAME" > "$EVIDENCE/container-name.txt"
docker inspect -f '{{.Id}}' "$NAME" > "$EVIDENCE/container.id"
docker inspect -f 'privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/hostconfig.txt"
