#!/bin/bash
# One private PostgreSQL 17.10 for the a394 review. Secrets stay under /tmp.
# No host port. No privileged mode. No password in the environment or argv.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a394
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP11/session-control-api-review/pg
ROOT=/tmp/oo-a394-${RUN_ID}
SEC=${ROOT}/secrets
FIX=${ROOT}/fixture
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a394-eef831aa-net
NAME=oo-a394-pg
DATAVOL=oo-a394-eef831aa-pgdata
PARENT=${REPO}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a387/SP11/session-control-api/pg

mkdir -p "$PG"
chmod 755 "$PG/pg-wrap.sh" "$PG/up.sh"
if [ -f "$PG/up.exit" ]; then
  echo "refusing to overwrite pg/up.exit" >&2
  exit 2
fi
{
  echo "postgres recipe origin"
  echo "parent_up=${PARENT}/up.sh"
  echo "parent_up_sha256=$(shasum -a 256 "$PARENT/up.sh" | awk '{print $1}')"
  echo "parent_wrap=${PARENT}/pg-wrap.sh"
  echo "parent_wrap_sha256=$(shasum -a 256 "$PARENT/pg-wrap.sh" | awk '{print $1}')"
  echo "wrap_sha256=$(shasum -a 256 "$PG/pg-wrap.sh" | awk '{print $1}')"
  echo "minimal_changes=uuid a394; database oo_a394; alias oo-a394-pg; same roles; network created here if absent"
} > "$PG/recipe-origin.txt"
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"
own_pg=$(docker ps -q --filter label=oo.owner=a394 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
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
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a394 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null

umask 077
mkdir -p "$SEC/init" "$FIX"
chmod 700 "$SEC" "$FIX"
for name in fixture-admin admin runtime cancel query api-admin api-runtime token-admin token-runtime; do
  openssl rand -hex 24 > "$SEC/${name}.pass"
  grep -Eq '^[0-9a-f]{48}$' "$SEC/${name}.pass"
done
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a394-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a394-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a394-pg
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
api_admin_pass=$(cat "$SEC/api-admin.pass")
api_runtime_pass=$(cat "$SEC/api-runtime.pass")
token_admin_pass=$(cat "$SEC/token-admin.pass")
token_runtime_pass=$(cat "$SEC/token-runtime.pass")
cat > "$SEC/init/01-roles.sql" <<EOF
CREATE ROLE oo_session_admin LOGIN PASSWORD '${admin_pass}';
CREATE ROLE oo_session_runtime LOGIN PASSWORD '${runtime_pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_fixture_query LOGIN PASSWORD '${query_pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_session_cancel LOGIN PASSWORD '${cancel_pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
GRANT oo_fixture_query TO oo_session_cancel;
CREATE ROLE oo_token_admin LOGIN PASSWORD '${token_admin_pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_token_runtime LOGIN PASSWORD '${token_runtime_pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_session_api_admin LOGIN PASSWORD '${api_admin_pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_session_api_runtime LOGIN PASSWORD '${api_runtime_pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE DATABASE oo_a394 OWNER oo_session_admin;
REVOKE ALL ON DATABASE oo_a394 FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE oo_a394 TO oo_session_admin;
GRANT CONNECT, CREATE ON DATABASE oo_a394 TO oo_token_admin;
GRANT CONNECT, CREATE ON DATABASE oo_a394 TO oo_session_api_admin;
GRANT CONNECT ON DATABASE oo_a394 TO oo_session_runtime;
GRANT CONNECT ON DATABASE oo_a394 TO oo_fixture_query;
GRANT CONNECT ON DATABASE oo_a394 TO oo_session_cancel;
GRANT CONNECT ON DATABASE oo_a394 TO oo_token_runtime;
GRANT CONNECT ON DATABASE oo_a394 TO oo_session_api_runtime;
EOF
unset admin_pass runtime_pass cancel_pass query_pass api_admin_pass api_runtime_pass token_admin_pass token_runtime_pass
chmod 600 "$SEC/init/01-roles.sql"
cp "$SEC/ca.crt" "$FIX/ca.crt"
for name in admin runtime cancel query api-admin api-runtime token-admin token-runtime; do
  cp "$SEC/${name}.pass" "$FIX/${name}.pass"
  chmod 600 "$FIX/${name}.pass"
done
printf '%s\n' oo-a394-pg 5432 oo_a394 > "$FIX/endpoint.txt"
chmod 644 "$FIX/ca.crt" "$FIX/endpoint.txt"

docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a394 \
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
  --network-alias oo-a394-pg \
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
  --label oo.owner=a394 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=session-control-api-review \
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
