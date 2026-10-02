#!/bin/bash
# One owned PostgreSQL 17.10 fixture. No pull, no host port, no prune.
set -euo pipefail
umask 077
RUN_ID="${1:?run id}"
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
ROOT="/tmp/oo-a098-${RUN_ID}"
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a098/SP03/lossless-types/pg
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a098
NAME="oo-a098-pg-${RUN_ID}"
NET="oo-a098-pg-${RUN_ID}-net"
VOL="oo-a098-pg-${RUN_ID}-data"
LOGVOL="oo-a098-pg-${RUN_ID}-log"

mkdir -p "$EVIDENCE/out" "$ROOT/secrets"
chmod 700 "$ROOT" "$ROOT/secrets"
if [ ! -f "$ROOT/secrets/admin.pass" ]; then
  openssl rand -hex 24 > "$ROOT/secrets/admin.pass"
fi
if [ ! -f "$ROOT/secrets/reader.pass" ]; then
  openssl rand -hex 24 > "$ROOT/secrets/reader.pass"
fi
chmod 600 "$ROOT/secrets/admin.pass" "$ROOT/secrets/reader.pass"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  exit 1
fi
own=$(docker ps -a --filter label=oo.owner=a098 --filter label=oo.kind=postgres --format '{{.Names}}')
if [ -n "$own" ]; then
  echo "this task already has a postgres fixture: $own" >&2
  exit 3
fi

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "foreign_postgres:"
  docker ps -a --filter label=oo.kind=postgres --format '{{.Names}} {{.Status}} {{.Label "oo.owner"}}'
} > "$EVIDENCE/out/postgres-before.txt"

openssl req -x509 -newkey rsa:2048 -nodes -keyout "$ROOT/secrets/ca.key" -out "$ROOT/secrets/ca.crt" -days 30 -subj "/CN=oo-a098-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$ROOT/secrets/server.key" -out "$ROOT/secrets/server.csr" -subj "/CN=pg.a098.internal" >/dev/null 2>&1
cat > "$ROOT/secrets/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:pg.a098.internal
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$ROOT/secrets/server.csr" -CA "$ROOT/secrets/ca.crt" -CAkey "$ROOT/secrets/ca.key" -CAcreateserial -out "$ROOT/secrets/server.crt" -days 30 -extfile "$ROOT/secrets/san.cnf" >/dev/null 2>&1
chmod 600 "$ROOT/secrets"/*

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a098 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  "$NET" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a098 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  "$VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a098 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  "$LOGVOL" >/dev/null

docker run -d \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add DAC_OVERRIDE \
  --cap-add FOWNER \
  --cap-add SETGID \
  --cap-add SETUID \
  --memory 1g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias pg.a098.internal \
  --tmpfs /tmp:rw,nosuid,nodev,size=64m \
  --mount "type=volume,source=${VOL},target=/var/lib/postgresql/data" \
  --mount "type=volume,source=${LOGVOL},target=/pglog" \
  --mount "type=bind,source=${ROOT}/secrets,target=/secrets,readonly" \
  --mount "type=bind,source=${ROOT}/secrets,target=/tls-in,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/sql,readonly" \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --label oo.project=open-ontology \
  --label oo.owner=a098 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  --entrypoint /sql/pg-wrap.sh \
  "$IMAGE" >/dev/null

printf '%s\n' "$RUN_ID" > "$EVIDENCE/out/run-id.txt"
printf '%s\n' "$NAME" > "$EVIDENCE/out/container-name.txt"
printf '%s\n' "$IMAGE" > "$EVIDENCE/out/image-ref.txt"
docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} security={{json .HostConfig.SecurityOpt}}' > "$EVIDENCE/out/hostconfig.txt"
docker inspect "$NAME" --format '{{range .Config.Env}}{{println .}}{{end}}' | awk -F= '{print $1}' > "$EVIDENCE/out/env-keys.txt"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24; do
  if docker exec "$NAME" gosu postgres pg_isready -U postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 5
done
printf '%s\n' "$ready" > "$EVIDENCE/out/ready.exit"
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  exit 1
fi
docker exec "$NAME" bash /sql/load.sh
docker exec "$NAME" gosu postgres psql -d typedb -At -f /sql/oracle.sql > "$EVIDENCE/out/oracle-raw.json"
python3 "$EVIDENCE/oracle.py" < "$EVIDENCE/out/oracle-raw.json" > "$EVIDENCE/out/oracle.json"
cp "$EVIDENCE/out/oracle.json" "$ROOT/secrets/oracle.json"
cp "$EVIDENCE/out/oracle-raw.json" "$ROOT/secrets/oracle-raw.json"
python3 - "$ROOT/secrets/pointer.json" "$EVIDENCE/out/pointer.json" <<'PY'
import json, sys
body = {
    "host": "pg.a098.internal",
    "port": 5432,
    "database": "typedb",
    "reader_user": "oo_type_reader",
    "password_file": "/fixture/reader.pass",
    "ca_file": "/fixture/ca.crt",
    "oracle_file": "/fixture/oracle.json",
    "raw_file": "/fixture/oracle-raw.json",
}
text = json.dumps(body, indent=2) + "\n"
for path in sys.argv[1:]:
    open(path, "w").write(text)
PY
chmod 600 "$ROOT/secrets/pointer.json" "$ROOT/secrets/oracle.json" "$ROOT/secrets/oracle-raw.json"
docker exec "$NAME" gosu postgres psql -d typedb -Atc "SHOW server_version" > "$EVIDENCE/out/server-version.txt"
docker exec "$NAME" gosu postgres psql -d typedb -Atc "SHOW server_version_num" > "$EVIDENCE/out/server-version-num.txt"
echo "provision_ready name=$NAME"
