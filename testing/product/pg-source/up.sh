#!/bin/bash
# Start one pinned PostgreSQL 17.10 container and load the synthetic catalog.
# Stdout is the fixture pointer path. Secret bodies stay under /tmp.
set -eu
ROOT="$(cd "$(dirname "$0")" && pwd)"
RUN="$(uuidgen | tr '[:upper:]' '[:lower:]')"
SEC="/tmp/oo-a066-secrets/$RUN"
NAME="oo-a066-pg-$RUN"
NET="oo-a066-pgnet-$RUN"
VOL="oo-a066-pglog-$RUN"
# Local image ID of the pinned 17.10 arm64 image. Untagged images are not
# addressable as postgres@sha256 (that form requires a registry RepoDigest).
IMAGE="sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
STATE="/tmp/oo-a066-pg.state"
POINTER="$SEC/pointer.json"

docker image inspect "$IMAGE" >/dev/null
umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/super.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl rand -hex 24 > "$SEC/inherit.pass"
openssl rand -hex 24 > "$SEC/setonly.pass"
openssl rand -hex 24 > "$SEC/definer.pass"
for role in side hiddenfn schemacreate dbcreate dual transitive noset column bypass createrole createdb oponly castonly guc; do
  openssl rand -hex 24 > "$SEC/$role.pass"
done
openssl rand -hex 24 > "$SEC/inherit-only.pass"
openssl rand -hex 24 > "$SEC/login-super.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 30 -subj "/CN=oo-a060-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-source-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-source-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 30 -extfile "$SEC/san.cnf" >/dev/null 2>&1
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/wrong-ca.key" -out "$SEC/wrong-ca.crt" -days 30 -subj "/CN=oo-a060-wrong-ca" >/dev/null 2>&1
chmod 600 "$SEC"/*

docker network create \
  --label project=open-ontology \
  --label owner=a066 \
  --label scope=pg-source-fix \
  --label run="$RUN" \
  "$NET" >/dev/null
docker volume create \
  --label project=open-ontology \
  --label owner=a066 \
  --label scope=pg-source-fix \
  --label run="$RUN" \
  "$VOL" >/dev/null

cat > "$STATE" <<EOF
run=$RUN
container=$NAME
network=$NET
volume=$VOL
secrets=$SEC
pointer=$POINTER
EOF

docker run -d --name "$NAME" \
  --platform linux/arm64 \
  --network "$NET" \
  --network-alias oo-source-pg \
  --network-alias oo-source-wrong \
  --memory 512m \
  --memory-swap 512m \
  --cpus 1 \
  --pids-limit 256 \
  --label project=open-ontology \
  --label owner=a066 \
  --label scope=pg-source-fix \
  --label run="$RUN" \
  -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=postgres \
  -v "$SEC":/secrets:ro \
  -v "$SEC":/tls-in:ro \
  -v "$VOL":/pglog \
  -v "$ROOT":/fixture:ro \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" >/dev/null

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec "$NAME" pg_isready -U postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  exit 1
fi
docker exec "$NAME" /fixture/load.sh

python3 - "$POINTER" <<'PY'
import json, sys
path = sys.argv[1]
body = {
    "host": "oo-source-pg",
    "port": 5432,
    "database": "source_lab",
    "empty_database": "source_empty",
    "reader_user": "oo_src_reader",
    "inherit_user": "oo_src_inherit",
    "setonly_user": "oo_src_setonly",
    "definer_user": "oo_src_definer",
    "side_user": "oo_src_side",
    "hiddenfn_user": "oo_src_hiddenfn",
    "schemacreate_user": "oo_src_schemacreate",
    "dbcreate_user": "oo_src_dbcreate",
    "dual_user": "oo_src_dual",
    "inherit_only_user": "oo_src_inherit_only",
    "transitive_user": "oo_src_transitive",
    "noset_user": "oo_src_noset",
    "column_user": "oo_src_column",
    "super_user": "oo_src_super",
    "bypass_user": "oo_src_bypass",
    "createrole_user": "oo_src_createrole",
    "createdb_user": "oo_src_createdb",
    "oponly_user": "oo_src_oponly",
    "castonly_user": "oo_src_castonly",
    "guc_user": "oo_src_guc_login",
    "password_dir": "/secrets",
    "ca_file": "/secrets/ca.crt",
    "wrong_ca_file": "/secrets/wrong-ca.crt",
    "wrong_host": "oo-source-wrong",
    "log_file": "/pglog/postgresql.log",
}
with open(path, "w", encoding="utf-8") as handle:
    json.dump(body, handle)
PY
chmod 600 "$POINTER"
echo "$POINTER"
