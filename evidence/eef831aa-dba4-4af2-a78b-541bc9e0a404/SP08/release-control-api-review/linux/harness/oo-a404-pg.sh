#!/bin/bash
# Private Postgres 17.10 for the a404 review. Does not print passwords.
# pg-wrap origin commit b91b67bee0f603725b2cb7cc9c284588a2699392
# path deploy/trusted-demo/pg/pg-wrap.sh
# sha256 05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a
set -euo pipefail
OWNER=a404
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a404
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE="$REPO/evidence/${RUN_ID}/SP08/release-control-api-review"
PG_EVIDENCE="$EVIDENCE/pg"
WRAP="$REPO/deploy/trusted-demo/pg/pg-wrap.sh"
WRAP_SHA=05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a
ROOT=/tmp/oo-a404-${RUN_ID}
NET=oo-a404-eef831aa-net
PG_NAME=oo-a404-pg
PG_HOST=oo-a404-pg
PG_VOL=oo-a404-eef831aa-pg
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb

mkdir -p "$PG_EVIDENCE" "$ROOT"
chmod 700 "$ROOT"
got=$(shasum -a 256 "$WRAP" | awk '{print $1}')
printf '%s\n' "$got" > "$PG_EVIDENCE/pg-wrap.sha256"
if [ "$got" != "$WRAP_SHA" ]; then
  echo "pg-wrap hash mismatch" >&2
  exit 2
fi
git -C "$REPO" rev-parse b91b67bee0f603725b2cb7cc9c284588a2699392 > "$PG_EVIDENCE/pg-wrap.origin"
if docker ps -a --format '{{.Names}}' | grep -qx "$PG_NAME"; then
  echo "own pg already present" >&2
  exit 2
fi
own_pg=$(docker ps -q --filter name=^/${PG_NAME}$ | wc -l | tr -d ' ')
printf 'own_pg=%s\n' "$own_pg" | tee "$PG_EVIDENCE/capacity-pg.txt"
if [ "$own_pg" != 0 ]; then
  exit 2
fi

umask 077
pass() { openssl rand -hex 24 | tr -d '\n' > "$ROOT/$1.pass"; }
pass admin
pass api
pass producer
pass reader
pass identity

openssl req -x509 -newkey rsa:2048 -nodes -keyout "$ROOT/ca.key" -out "$ROOT/ca.pem" -days 30 -subj "/CN=oo-a404-https-ca" -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$ROOT/srv.key" -out "$ROOT/srv.csr" -subj "/CN=127.0.0.1" >/dev/null 2>&1
printf '%s\n' "basicConstraints=CA:FALSE" "keyUsage=digitalSignature,keyEncipherment" "extendedKeyUsage=serverAuth" "subjectAltName=IP:127.0.0.1" > "$ROOT/srv.ext"
openssl x509 -req -in "$ROOT/srv.csr" -CA "$ROOT/ca.pem" -CAkey "$ROOT/ca.key" -CAcreateserial -out "$ROOT/srv.pem" -days 30 -extfile "$ROOT/srv.ext" >/dev/null 2>&1
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$ROOT/k1.pem" >/dev/null 2>&1
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$ROOT/k2.pem" >/dev/null 2>&1
openssl pkcs8 -topk8 -nocrypt -in "$ROOT/k1.pem" -outform DER -out "$ROOT/k1.der"
openssl pkcs8 -topk8 -nocrypt -in "$ROOT/k2.pem" -outform DER -out "$ROOT/k2.der"
rm -f "$ROOT/k1.pem" "$ROOT/k2.pem" "$ROOT/srv.csr" "$ROOT/ca.key" "$ROOT/ca.srl"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$ROOT/pg-ca.key" -out "$ROOT/ca.crt" -days 30 -subj "/CN=oo-a404-pg-ca" -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$ROOT/server.key" -out "$ROOT/pg.csr" -subj "/CN=oo-a404-pg" >/dev/null 2>&1
printf '%s\n' "basicConstraints=CA:FALSE" "keyUsage=digitalSignature,keyEncipherment" "extendedKeyUsage=serverAuth" "subjectAltName=DNS:oo-a404-pg" > "$ROOT/pg.ext"
openssl x509 -req -in "$ROOT/pg.csr" -CA "$ROOT/ca.crt" -CAkey "$ROOT/pg-ca.key" -CAcreateserial -out "$ROOT/server.crt" -days 30 -extfile "$ROOT/pg.ext" >/dev/null 2>&1
rm -f "$ROOT/pg.csr" "$ROOT/pg-ca.key" "$ROOT/ca.srl" "$ROOT/srv.ext" "$ROOT/pg.ext"
chmod 600 "$ROOT"/*

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create --driver bridge "$NET" >/dev/null
fi
docker volume create "$PG_VOL" >/dev/null
docker image inspect "$IMAGE" --format '{{.Os}}/{{.Architecture}} {{.Id}}' > "$PG_EVIDENCE/image.txt"

docker run -d --name "$PG_NAME" \
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
  --network-alias "$PG_HOST" \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${ROOT},target=/secrets,readonly" \
  --mount "type=bind,source=${ROOT},target=/tls-in,readonly" \
  --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=$(dirname "$WRAP"),target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.kind=postgres \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" > "$PG_EVIDENCE/container.id"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$PG_NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" != 1 ]; then
  docker logs "$PG_NAME" > "$PG_EVIDENCE/postgres.log" 2>&1 || true
  echo "postgres did not become ready" >&2
  printf '%s\n' 1 > "$PG_EVIDENCE/up.exit"
  exit 1
fi

api=$(cat "$ROOT/api.pass")
producer=$(cat "$ROOT/producer.pass")
reader=$(cat "$ROOT/reader.pass")
identity=$(cat "$ROOT/identity.pass")
docker exec -i -u postgres "$PG_NAME" psql -U oo_fixture_admin -d postgres -v ON_ERROR_STOP=1 > "$PG_EVIDENCE/bootstrap.out" 2> "$PG_EVIDENCE/bootstrap.err" <<SQL
CREATE ROLE oo_identity_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_identity_admin LOGIN PASSWORD '${identity}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_release_reader LOGIN PASSWORD '${reader}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_release_admin LOGIN PASSWORD '${producer}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE ROLE oo_release_api_admin LOGIN PASSWORD '${api}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
CREATE DATABASE oo_a404 OWNER oo_fixture_admin;
CREATE DATABASE oo_a404_csrf OWNER oo_fixture_admin;
GRANT CREATE ON DATABASE oo_a404 TO oo_identity_admin;
GRANT CREATE ON DATABASE oo_a404 TO oo_release_admin;
GRANT CREATE ON DATABASE oo_a404 TO oo_release_api_admin;
GRANT CREATE ON DATABASE oo_a404_csrf TO oo_identity_admin;
SQL
unset api producer reader identity
for name in api producer reader identity admin; do
  if grep -F -q -f "$ROOT/${name}.pass" "$PG_EVIDENCE/bootstrap.out" "$PG_EVIDENCE/bootstrap.err"; then
    rm -f "$PG_EVIDENCE/bootstrap.out" "$PG_EVIDENCE/bootstrap.err"
    echo "bootstrap output contained a password" >&2
    printf '%s\n' 2 > "$PG_EVIDENCE/up.exit"
    exit 2
  fi
done

docker exec -u postgres "$PG_NAME" psql -U oo_fixture_admin -d postgres -tAc 'SHOW server_version' > "$PG_EVIDENCE/server-version.txt"
docker inspect "$PG_NAME" --format 'id={{.Id}} status={{.State.Status}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} memory={{.HostConfig.Memory}} cpus={{.HostConfig.NanoCpus}}' > "$PG_EVIDENCE/inspect.txt"
docker inspect "$PG_NAME" --format '{{json .HostConfig.PortBindings}}' > "$PG_EVIDENCE/ports.json"
docker inspect "$PG_NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PG_EVIDENCE/caps.json"
printf '%s\n' 0 > "$PG_EVIDENCE/up.exit"
echo postgres_ready
