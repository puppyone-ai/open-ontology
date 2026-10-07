#!/bin/bash
# One private PostgreSQL 17.10. Recipe flags from testing/real-db/gateway-tls/pg/up.sh.
# a364 names. No host port, no pull, no prune.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
OUT=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a364/SP13/selfbuild-suggestions-review/pg
SEC=/tmp/oo-a364-sbs-secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a364-sbs-net
NAME=oo-a364-pg
DATAVOL=oo-a364-pgdata
WRAP=${SRC}/testing/real-db/gateway-tls/pg/pg-wrap.sh
MIGRATIONS=/tmp/oo-a364-sbs-archive/crates/oo-pipeline/migrations
BASE=7a6f7eac8b8041eae215c087d1a80930437871ce

mkdir -p "$OUT"
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$OUT/image.txt"
grep -q 'arch=arm64' "$OUT/image.txt"
own_pg=$(docker ps -q --filter label=oo.owner=a364 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own_pg" > "$OUT/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$OUT/up.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.Status}}' | grep -q .; then
  echo "pg container name already exists" >&2
  printf '%s\n' 2 > "$OUT/up.exit"
  exit 2
fi
mkdir -p /tmp/oo-a364-sbs-archive
if [ ! -f "$MIGRATIONS/0005_full_import_expand.sql" ]; then
  git -C "$SRC" archive --format=tar "$BASE" crates/oo-pipeline/migrations | tar -x -C /tmp/oo-a364-sbs-archive
fi
if [ ! -f "$MIGRATIONS/0005_full_import_expand.sql" ]; then
  echo "fixed-base migrations are not archived" >&2
  printf '%s\n' 2 > "$OUT/up.exit"
  exit 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create --driver bridge "$NET" >/dev/null
umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/login.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a364-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a364-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a364-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$OUT/server-san.txt"
if find "$OUT" -type f \( -name '*.key' -o -name '*.pass' -o -name 'server.crt' \) | grep -q .; then
  echo "evidence pg dir contains secret material" >&2
  printf '%s\n' 2 > "$OUT/up.exit"
  exit 2
fi
docker volume inspect "$DATAVOL" >/dev/null 2>&1 || docker volume create \
  --label oo.owner=a364 \
  --label oo.kind=postgres \
  "$DATAVOL" > "$OUT/volume-data.txt"
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
  --network-alias oo-a364-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_a364_admin \
  -e POSTGRES_DB=oo_a364 \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${WRAP},target=/fixture/pg-wrap.sh,readonly" \
  --mount "type=bind,source=${MIGRATIONS},target=/migrations,readonly" \
  --mount "type=bind,source=${OUT}/apply.sh,target=/fixture/apply.sh,readonly" \
  --label oo.owner=a364 \
  --label oo.kind=postgres \
  --entrypoint bash \
  "$IMAGE" /fixture/pg-wrap.sh > "$OUT/container.id"
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$NAME" pg_isready -U oo_a364_admin -d oo_a364 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" != 1 ]; then
  echo "postgres did not become ready" >&2
  printf '%s\n' 1 > "$OUT/up.exit"
  exit 1
fi
printf '%s\n' 0 > "$OUT/up.exit"
echo "postgres ready"
