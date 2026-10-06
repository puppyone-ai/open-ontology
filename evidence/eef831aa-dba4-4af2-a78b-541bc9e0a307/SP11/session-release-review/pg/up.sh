#!/bin/bash
# One private PostgreSQL 17.10 for the session-release review. Secrets stay under /tmp.
# No host port. No privileged mode. No password in the environment or argv.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a307
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP11/session-release-review/pg
ROOT=/tmp/oo-a307-${RUN_ID}
SEC=${ROOT}/secrets
FIX=${ROOT}/fixture
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a307-eef831aa-net
NAME=oo-a307-pg
DATAVOL=oo-a307-eef831aa-pgdata

mkdir -p "$PG"
chmod 755 "$PG/pg-wrap.sh" "$PG/up.sh"
if [ -f "$PG/up.exit" ]; then
  echo "refusing to overwrite pg/up.exit" >&2
  exit 2
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"
own_pg=$(docker ps -q --filter label=oo.owner=a307 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
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
    --label oo.owner=a307 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" >/dev/null
fi
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$PG/network.txt"

umask 077
mkdir -p "$SEC/init" "$FIX"
chmod 700 "$SEC" "$FIX"
openssl rand -hex 24 > "$SEC/fixture-admin.pass"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/runtime.pass"
openssl rand -hex 24 > "$SEC/cancel.pass"
openssl rand -hex 24 > "$SEC/query.pass"
grep -Eq '^[0-9a-f]{48}$' "$SEC/admin.pass"
grep -Eq '^[0-9a-f]{48}$' "$SEC/runtime.pass"
grep -Eq '^[0-9a-f]{48}$' "$SEC/cancel.pass"
grep -Eq '^[0-9a-f]{48}$' "$SEC/query.pass"
grep -Eq '^[0-9a-f]{48}$' "$SEC/fixture-admin.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a307-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a307-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a307-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
find "$SEC" -maxdepth 1 -type f -exec chmod 600 {} +
chmod 700 "$SEC/init"
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$PG/server-san.txt"
admin_pass=$(cat "$SEC/admin.pass")
runtime_pass=$(cat "$SEC/runtime.pass")
cancel_pass=$(cat "$SEC/cancel.pass")
query_pass=$(cat "$SEC/query.pass")
cat > "$SEC/init/01-roles.sql" <<EOF
CREATE ROLE oo_session_admin LOGIN PASSWORD '${admin_pass}';
CREATE ROLE oo_session_runtime LOGIN PASSWORD '${runtime_pass}';
CREATE ROLE oo_fixture_query LOGIN PASSWORD '${query_pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_session_cancel LOGIN PASSWORD '${cancel_pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
GRANT oo_fixture_query TO oo_session_cancel;
CREATE DATABASE oo_a307 OWNER oo_session_admin;
REVOKE ALL ON DATABASE oo_a307 FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE oo_a307 TO oo_session_admin;
GRANT CONNECT ON DATABASE oo_a307 TO oo_session_runtime;
GRANT CONNECT ON DATABASE oo_a307 TO oo_fixture_query;
GRANT CONNECT ON DATABASE oo_a307 TO oo_session_cancel;
EOF
unset admin_pass runtime_pass cancel_pass query_pass
chmod 600 "$SEC/init/01-roles.sql"
cp "$SEC/ca.crt" "$FIX/ca.crt"
cp "$SEC/admin.pass" "$FIX/admin.pass"
cp "$SEC/runtime.pass" "$FIX/runtime.pass"
cp "$SEC/cancel.pass" "$FIX/cancel.pass"
cp "$SEC/query.pass" "$FIX/query.pass"
printf '%s\n' oo-a307-pg 5432 oo_a307 > "$FIX/endpoint.txt"
chmod 644 "$FIX/ca.crt" "$FIX/endpoint.txt"
chmod 600 "$FIX/admin.pass" "$FIX/runtime.pass" "$FIX/cancel.pass" "$FIX/query.pass"

docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a307 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  "$DATAVOL" > "$PG/volume-data.txt"

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
  --network-alias oo-a307-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/fixture-admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=bind,source=${SEC}/init,target=/docker-entrypoint-initdb.d,readonly" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${PG},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a307 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=session-release-review \
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
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$PG/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$PG/ports.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PG/caps.json"
printf '%s\n' 0 > "$PG/up.exit"
echo "postgres_ready name=$NAME"
