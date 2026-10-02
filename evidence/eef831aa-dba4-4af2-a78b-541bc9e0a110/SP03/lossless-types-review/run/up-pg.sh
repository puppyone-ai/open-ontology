#!/bin/bash
# One owned PostgreSQL 17.10 fixture. No pull, no published port, no prune.
set -euo pipefail
umask 077
EV=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a110/SP03/lossless-types-review
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a110
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
RUN_ID=$(uuidgen | tr '[:upper:]' '[:lower:]')
ROOT="/tmp/oo-a110-${RUN_ID}"
NAME="oo-a110-pg-${RUN_ID}"
NET="oo-a110-pg-${RUN_ID}-net"
VOL="oo-a110-pg-${RUN_ID}-data"
LOGVOL="oo-a110-pg-${RUN_ID}-log"

host_compilers=$(ps -axo comm= | awk '$1=="rustc" || $1=="cargo" || $1 ~ /^cargo-/ { c++ } END { print c+0 }')
running=$(docker ps -q | wc -l | tr -d ' ')
printf 'host_compilers=%s\nrunning_containers=%s\n' "$host_compilers" "$running" | tee "$EV/raw/admin/capacity-before-pg.txt"
if [ "$host_compilers" -ne 0 ] || [ "$running" -ne 0 ]; then
  echo "builder capacity is not free" >&2
  exit 3
fi
own=$(docker ps -a --filter label=oo.owner=a110 --filter label=oo.kind=postgres --format '{{.Names}}')
if [ -n "$own" ]; then
  echo "this task already has a postgres fixture: $own" >&2
  exit 3
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  exit 1
fi

mkdir -p "$ROOT/secrets" "$EV/raw/admin" "$EV/raw/phases" "$EV/raw/attempts"
chmod 700 "$ROOT" "$ROOT/secrets"
openssl rand -hex 24 > "$ROOT/secrets/admin.pass"
openssl rand -hex 24 > "$ROOT/secrets/reader.pass"
openssl rand -hex 24 > "$ROOT/secrets/guc.pass"
openssl rand -hex 24 > "$ROOT/secrets/alias.pass"
chmod 600 "$ROOT/secrets/"*.pass
printf '%s\n' "$RUN_ID" > "$EV/raw/admin/run-id.txt"
printf '%s\n' "$NAME" > "$EV/raw/admin/container-name.txt"
printf '%s\n' "$ROOT" > "$EV/raw/admin/secret-root.txt"

openssl req -x509 -newkey rsa:2048 -nodes -keyout "$ROOT/secrets/ca.key" -out "$ROOT/secrets/ca.crt" -days 30 -subj "/CN=oo-a110-ca" > "$EV/raw/admin/openssl-ca.out" 2>&1
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$ROOT/secrets/wrong-ca.key" -out "$ROOT/secrets/wrong-ca.crt" -days 30 -subj "/CN=oo-a110-wrong-ca" > "$EV/raw/admin/openssl-wrong-ca.out" 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$ROOT/secrets/server.key" -out "$ROOT/secrets/server.csr" -subj "/CN=pg.a110.internal" > "$EV/raw/admin/openssl-csr.out" 2>&1
cat > "$ROOT/secrets/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:pg.a110.internal
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$ROOT/secrets/server.csr" -CA "$ROOT/secrets/ca.crt" -CAkey "$ROOT/secrets/ca.key" -CAcreateserial -out "$ROOT/secrets/server.crt" -days 30 -extfile "$ROOT/secrets/san.cnf" > "$EV/raw/admin/openssl-server.out" 2>&1
openssl x509 -in "$ROOT/secrets/server.crt" -noout -subject -ext subjectAltName > "$EV/raw/admin/server-san.txt"
chmod 600 "$ROOT/secrets/"*

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a110 \
  --label oo.session="$SESSION" \
  --label oo.run_id="$RUN_ID" \
  --label oo.kind=postgres \
  "$NET" > "$EV/raw/admin/network-id.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a110 \
  --label oo.session="$SESSION" \
  --label oo.run_id="$RUN_ID" \
  --label oo.kind=postgres \
  "$VOL" > "$EV/raw/admin/data-volume.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a110 \
  --label oo.session="$SESSION" \
  --label oo.run_id="$RUN_ID" \
  --label oo.kind=postgres \
  "$LOGVOL" > "$EV/raw/admin/log-volume.txt"

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
  --network-alias pg.a110.internal \
  --network-alias wrong.a110.internal \
  --tmpfs /tmp:rw,nosuid,nodev,size=64m \
  --mount "type=volume,source=${VOL},target=/var/lib/postgresql/data" \
  --mount "type=volume,source=${LOGVOL},target=/pglog" \
  --mount "type=bind,source=${ROOT}/secrets,target=/secrets,readonly" \
  --mount "type=bind,source=${ROOT}/secrets,target=/tls-in,readonly" \
  --mount "type=bind,source=${EV},target=/sql,readonly" \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --label oo.project=open-ontology \
  --label oo.owner=a110 \
  --label oo.session="$SESSION" \
  --label oo.run_id="$RUN_ID" \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  --entrypoint /sql/fixture/pg-wrap.sh \
  "$IMAGE" > "$EV/raw/admin/container-id.txt"

docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} security={{json .HostConfig.SecurityOpt}} memory={{.HostConfig.Memory}}' > "$EV/raw/admin/hostconfig.txt"
docker inspect "$NAME" --format '{{range .Config.Env}}{{println .}}{{end}}' | awk -F= '{print $1}' > "$EV/raw/admin/env-keys.txt"
docker image inspect "$IMAGE" --format '{{.Id}} {{.Os}}/{{.Architecture}}' > "$EV/raw/admin/image.txt"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec "$NAME" gosu postgres pg_isready -U postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
printf '%s\n' "$ready" > "$EV/raw/admin/ready.exit"
if [ "$ready" -ne 1 ]; then
  docker logs "$NAME" > "$EV/raw/admin/postgres-not-ready.log" 2>&1 || true
  echo "postgres did not become ready" >&2
  exit 1
fi

docker exec "$NAME" bash /sql/fixture/seed.sh > "$EV/raw/admin/seed-host.out" 2> "$EV/raw/admin/seed-host.err"
docker cp "$NAME":/pglog/admin "$EV/raw/admin/pg-admin"
cp "$ROOT/secrets/ca.crt" "$EV/raw/admin/ca.crt"
cp "$ROOT/secrets/wrong-ca.crt" "$EV/raw/admin/wrong-ca.crt"
cp "$ROOT/secrets/facts.json" "$EV/raw/phases/facts.json" 2>/dev/null || cp "$EV/raw/admin/pg-admin/facts.json" "$EV/raw/phases/facts.json"
cat > "$ROOT/secrets/pointer.json" <<EOF
{
  "host": "pg.a110.internal",
  "port": 5432,
  "database": "typedb",
  "reader_user": "oo_a110_reader",
  "guc_user": "oo_a110_guc",
  "alias_user": "oo_a110_alias",
  "password_file": "/fixture/reader.pass",
  "guc_password_file": "/fixture/guc.pass",
  "alias_password_file": "/fixture/alias.pass",
  "ca_file": "/fixture/ca.crt",
  "wrong_ca_file": "/fixture/wrong-ca.crt",
  "wrong_host": "wrong.a110.internal",
  "facts_file": "/fixture/facts.json"
}
EOF
cp "$EV/raw/admin/pg-admin/facts.json" "$ROOT/secrets/facts.json"
chmod 600 "$ROOT/secrets/pointer.json" "$ROOT/secrets/facts.json"
cp "$ROOT/secrets/pointer.json" "$EV/raw/admin/pointer.json"
echo "pg_ready name=$NAME run=$RUN_ID"
