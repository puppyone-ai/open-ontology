#!/bin/bash
# One owned PostgreSQL 17.10. Secrets stay under /var/tmp. No host port.
# Origin: a334 pg-3/up.sh. Name, network, and volume are a346-private.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a346
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP11/actual-model-receipt-review/pg
SEC=/var/tmp/oo-a346-${RUN_ID}/secrets
PUB=/var/tmp/oo-a346-${RUN_ID}/fixture
A334=${REPO}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/pg-3/up.sh
if [ -x /opt/homebrew/opt/openssl@3/bin/openssl ]; then
  OPENSSL=/opt/homebrew/opt/openssl@3/bin/openssl
else
  echo "openssl 3 is required" >&2
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a346-eef831aa-net
NAME=oo-a346-pg
DATAVOL=oo-a346-eef831aa-pgdata

mkdir -p "$PG"
chmod 755 "$PG/pg-wrap.sh" "$PG/up.sh"
if [ -f "$PG/up.exit" ]; then
  echo "refusing to overwrite pg up.exit" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"
{
  echo "a334 pg-3 up ${A334} $(shasum -a 256 "$A334" | awk '{print $1}')"
  echo "a334 pg-3 wrap reused byte-for-byte in this directory"
  echo "changes owner a346 network ${NET} no-host-port memory=1g cpus=1"
} > "$PG/recipe-origins.txt"

own_pg=$(docker ps -q --filter label=oo.owner=a346 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s cap_own=1\n' "$own_pg" > "$PG/capacity.txt"
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
    --label oo.owner=a346 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" >/dev/null
fi

umask 077
mkdir -p "$SEC" "$PUB"
chmod 700 "$SEC" "$PUB"
"$OPENSSL" rand -hex 24 > "$SEC/admin.pass"
"$OPENSSL" rand -hex 24 > "$SEC/login.pass"
"$OPENSSL" req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a346-ca" >/dev/null 2>&1
"$OPENSSL" req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a346-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a346-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
"$OPENSSL" x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
chmod 600 "$SEC/server.key" "$SEC/ca.key" "$SEC/admin.pass" "$SEC/login.pass"
cp "$SEC/ca.crt" "$PUB/ca.crt"
cp "$SEC/admin.pass" "$PUB/admin.pass"
cp "$SEC/login.pass" "$PUB/login.pass"
chmod 644 "$PUB/ca.crt"
chmod 600 "$PUB/admin.pass" "$PUB/login.pass"
printf '%s\n' "$PUB" > "$PG/fixture-dir.txt"
"$OPENSSL" x509 -in "$SEC/server.crt" -noout -subject -text | awk '/Subject:|Subject Alternative Name|DNS:/' > "$PG/server-san.txt"

if ! docker volume inspect "$DATAVOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a346 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=postgres \
    "$DATAVOL" > "$PG/volume-data.txt"
else
  docker volume inspect -f '{{.Name}}' "$DATAVOL" > "$PG/volume-data.txt"
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
  --network-alias oo-a346-pg \
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
  --label oo.owner=a346 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=actual-model-receipt-review \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" > "$PG/container.id"

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
  docker logs "$NAME" > "$PG/postgres-start.log" 2>&1 || true
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker exec -u postgres "$NAME" psql -U oo_fixture_admin -d postgres -Atc 'SHOW server_version' > "$PG/version.txt"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$PG/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$PG/ports.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PG/caps.json"
printf '%s\n' 0 > "$PG/up.exit"
echo "postgres_ready name=$NAME"
