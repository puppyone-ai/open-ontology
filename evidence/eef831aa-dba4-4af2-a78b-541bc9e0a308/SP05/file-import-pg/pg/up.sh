#!/bin/bash
# One owned PostgreSQL 17.10. Secrets stay under /tmp. No host port. No privileged mode.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a301/SP04/cdc-apply-pg/pg/up.sh
# which records a200 blob 96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45.
# Changes: a308 names, SAN DNS:oo-a308-pg, agent.pass, reuse an existing secret set and
# data volume, and copy only the client files into a directory the proof builder mounts.
# Role and database creation stay in the live test. This script does not print passwords.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a308
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP05/file-import-pg/pg
ROOT=/tmp/oo-a308-${RUN_ID}
SEC=${ROOT}/secrets
CLIENT=${ROOT}/client-secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a308-eef831aa-net
NAME=oo-a308-pg
DATAVOL=oo-a308-eef831aa-pgdata

mkdir -p "$PG"
chmod 755 "$PG/pg-wrap.sh" "$PG/up.sh"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"

own_pg=$(docker ps -q --filter label=oo.owner=a308 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own_pg" > "$PG/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$PG/up.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.Status}}' | grep -q .; then
  echo "pg container name already exists" >&2
  printf '%s\n' 2 > "$PG/up.exit"
  exit 2
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a308 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" > "$PG/network-create.txt"
fi
docker network inspect --format 'id={{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$PG/network.txt"

secrets_ready=0
if [ -f "$SEC/admin.pass" ] && [ -f "$SEC/runtime.pass" ] && [ -f "$SEC/agent.pass" ] \
  && [ -f "$SEC/ca.crt" ] && [ -f "$SEC/server.crt" ] && [ -f "$SEC/server.key" ]; then
  secrets_ready=1
fi
if [ "$secrets_ready" -eq 0 ] && docker volume inspect "$DATAVOL" >/dev/null 2>&1; then
  echo "data volume exists without a complete secret set" >&2
  printf '%s\n' 2 > "$PG/up.exit"
  exit 2
fi
umask 077
mkdir -p "$SEC" "$CLIENT"
chmod 700 "$SEC" "$CLIENT"
if [ "$secrets_ready" -eq 0 ]; then
  openssl rand -hex 24 > "$SEC/admin.pass"
  openssl rand -hex 24 > "$SEC/runtime.pass"
  openssl rand -hex 24 > "$SEC/agent.pass"
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a308-ca" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a308-pg" >/dev/null 2>&1
  cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a308-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
fi
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
chmod 600 "$SEC/server.key" "$SEC/ca.key" "$SEC/admin.pass" "$SEC/runtime.pass" "$SEC/agent.pass"
cp "$SEC/ca.crt" "$CLIENT/ca.crt"
cp "$SEC/admin.pass" "$CLIENT/admin.pass"
cp "$SEC/runtime.pass" "$CLIENT/runtime.pass"
cp "$SEC/agent.pass" "$CLIENT/agent.pass"
chmod 644 "$CLIENT/ca.crt"
chmod 600 "$CLIENT/admin.pass" "$CLIENT/runtime.pass" "$CLIENT/agent.pass"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$PG/server-san.txt"
printf '%s\n' "$secrets_ready" > "$PG/secrets-reused.txt"

if ! docker volume inspect "$DATAVOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a308 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=postgres \
    "$DATAVOL" > "$PG/volume-data.txt"
else
  docker volume inspect --format '{{.Name}}' "$DATAVOL" > "$PG/volume-data.txt"
fi

docker run -d --name "$NAME" \
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
  --network-alias oo-a308-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${PG},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a308 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=file-import-pg \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" > "$PG/container.id"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40; do
  if docker exec -u postgres "$NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  docker logs "$NAME" > "$PG/postgres-start.log" 2>&1 || true
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$PG/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$PG/ports.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PG/caps.json"
printf '%s\n' 0 > "$PG/up.exit"
echo "postgres_ready name=$NAME"
