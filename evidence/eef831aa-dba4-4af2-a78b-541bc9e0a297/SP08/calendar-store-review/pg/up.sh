#!/bin/bash
# One private PostgreSQL 17.10 for the a297 calendar-store review.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh
# sha256 96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45
# Changes: a297 names, calendar roles, no host port.
# The database name is oo_a289 because the frozen author test asserts that name.
# The server, network, volume, certificate, and passwords are a297-private.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a297
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP08/calendar-store-review/pg
ROOT=/tmp/oo-a297-${RUN_ID}
SEC=${ROOT}/secrets
FIX=${ROOT}/fixture
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a297-eef831aa-net
NAME=oo-a297-pg
DATAVOL=oo-a297-eef831aa-pgdata

mkdir -p "$PG"
chmod 755 "$PG/pg-wrap.sh" "$PG/up.sh"
if [ -f "$PG/up.exit" ]; then
  echo "refusing to overwrite pg/up.exit" >&2
  exit 2
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"
own_pg=$(docker ps -q --filter label=oo.owner=a297 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
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
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a297 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null

umask 077
mkdir -p "$SEC/init" "$FIX"
chmod 700 "$SEC" "$FIX"
openssl rand -hex 24 > "$SEC/fixture-admin.pass"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl rand -hex 24 > "$SEC/other.pass"
grep -Eq '^[0-9a-f]{48}$' "$SEC/admin.pass"
grep -Eq '^[0-9a-f]{48}$' "$SEC/reader.pass"
grep -Eq '^[0-9a-f]{48}$' "$SEC/other.pass"
grep -Eq '^[0-9a-f]{48}$' "$SEC/fixture-admin.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a297-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a297-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a297-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/wrong.key" -out "$SEC/wrong.crt" -days 7 -subj "/CN=oo-a297-wrong" >/dev/null 2>&1
find "$SEC" -maxdepth 1 -type f -exec chmod 600 {} +
chmod 700 "$SEC/init"
chmod 644 "$SEC/ca.crt" "$SEC/server.crt" "$SEC/wrong.crt"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$PG/server-san.txt"
admin_pass=$(cat "$SEC/admin.pass")
reader_pass=$(cat "$SEC/reader.pass")
other_pass=$(cat "$SEC/other.pass")
cat > "$SEC/init/01-roles.sql" <<EOF
CREATE ROLE oo_calendar_view NOLOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_calendar_admin LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${admin_pass}';
GRANT oo_calendar_view TO oo_calendar_admin;
CREATE ROLE oo_r_calendar LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${reader_pass}';
CREATE ROLE oo_r_other LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE PASSWORD '${other_pass}';
CREATE ROLE oo_business_owner NOLOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
CREATE DATABASE oo_a289 OWNER oo_fixture_admin;
REVOKE ALL ON DATABASE oo_a289 FROM PUBLIC;
GRANT CONNECT, CREATE ON DATABASE oo_a289 TO oo_calendar_admin;
GRANT CONNECT ON DATABASE oo_a289 TO oo_r_calendar;
GRANT CONNECT ON DATABASE oo_a289 TO oo_r_other;
EOF
unset admin_pass reader_pass other_pass
chmod 600 "$SEC/init/01-roles.sql"
cp "$SEC/ca.crt" "$FIX/ca.crt"
cp "$SEC/wrong.crt" "$FIX/wrong.crt"
cp "$SEC/admin.pass" "$FIX/admin.pass"
cp "$SEC/reader.pass" "$FIX/reader.pass"
cp "$SEC/other.pass" "$FIX/other.pass"
cp "$SEC/fixture-admin.pass" "$FIX/fixture-admin.pass"
printf '%s\n' oo-a297-pg 5432 oo_a289 > "$FIX/endpoint.txt"
chmod 644 "$FIX/ca.crt" "$FIX/wrong.crt" "$FIX/endpoint.txt"
chmod 600 "$FIX/admin.pass" "$FIX/reader.pass" "$FIX/other.pass" "$FIX/fixture-admin.pass"

docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a297 \
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
  --network-alias oo-a297-pg \
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
  --label oo.owner=a297 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=calendar-store-review \
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
  docker logs "$NAME" 2>&1 | sed -e "s/PASSWORD '[^']*'/PASSWORD 'redacted'/g" > "$PG/postgres-start.log" || true
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$PG/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$PG/ports.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PG/caps.json"
printf '%s\n' 0 > "$PG/up.exit"
echo "postgres_ready name=$NAME"
